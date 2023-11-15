use futures::StreamExt;
use futures::FutureExt;
use btleplug::api::{Central, Manager as _, Peripheral as _, WriteType, Characteristic};
use btleplug::platform::{Manager, Peripheral, Adapter};
use tokio::time;
use tokio::time::Duration;
use uuid::{uuid, Uuid};
use indicatif::{ProgressBar, ProgressStyle};

use std::fs;
use std::io::Write;
use std::path::Path;
use std::error::Error;
use std::convert::TryFrom;

use crate::flipper_pb;
use crate::protobuf_codec::{PROTOBUF_CHUNK_SIZE, ProtobufCodec};

// the uuid that we write to
const FLIPPER_RX_CHR_UUID: Uuid = uuid!("19ed82ae-ed21-4c9d-4145-228e62fe0000");
// the uuid that we read from
const FLIPPER_TX_CHR_UUID: Uuid = uuid!("19ed82ae-ed21-4c9d-4145-228e61fe0000");
// flow control
const FLIPPER_FLOW_CTRL_CHR_UUID: Uuid = uuid!("19ed82ae-ed21-4c9d-4145-228e63fe0000");

/// Representation of a Flipper device connected over Bluetooth LE
pub struct FlipperBle {
    flipper: Peripheral,
    proto: ProtobufCodec,
}

// prints a &[u8] in a style very similar to a Python bytearray
// from https://stackoverflow.com/a/41450295
use std::ascii::escape_default;
fn format_u8_slice(bs: &[u8]) -> String {
    let mut visible = String::new();
    for &b in bs {
        let part: Vec<u8> = escape_default(b).collect();
        visible.push_str(std::str::from_utf8(&part).unwrap());
    }
    visible
}

impl FlipperBle {
    #[cfg(target_os = "windows")]
    async fn flipper_scan(central: &Adapter) -> Result<(), Box<dyn Error>> {
        use btleplug::api::ScanFilter;
        // Flipper doesn't advertise the serial service, so we just
        // scan. I've tested 5 seconds on several Intel cards
        // (including the broken ones) and it seems to work fine.
        central.start_scan(ScanFilter::default()).await?;
        // The event stream probably isn't useful because it only
        // shows MAC address, and if we don't know it already, that's
        // not useful.
        time::sleep(Duration::from_millis(5000)).await;
        // stop scanning to connect
        central.stop_scan().await?;
        Ok(())
    }
    
    async fn find_device_named(flipper_name: &str, central: &Adapter) -> Option<Peripheral> {
        for p in central.peripherals().await.unwrap() {
            if p.properties()
                .await
                .unwrap()
                .unwrap()
                .local_name
                .iter()
                .any(|name| name.contains(flipper_name)) {
                    info!("found Flipper {}", flipper_name);
                    debug!("peripheral details: {:?}", p);
                    return Some(p);
                }
        }
        None
    }
    
    /// Returns a new FlipperBle with the discovered device connected,
    /// or an error if no device was found or other error
    /// occurred. The Flipper must already be known to the system
    /// (i.e., already paired).
    ///
    /// # Arguments
    ///
    /// * `flipper_name`: Search pattern (usually a Flipper name) to
    ///                   find in the list of discovered devices
    pub async fn connect_paired_device(flipper_name: &str) -> Result<FlipperBle, Box<dyn Error>> {
        let manager = Manager::new().await?;
        let central = match manager.adapters().await {
            Ok(adapters) => {
                if adapters.is_empty() {
                    return Err("no Bluetooth adapters found".into());
                }
                adapters.into_iter().nth(0).unwrap()
            },
            Err(e) => {
                return Err(format!("error finding Bluetooth adapters: {:?}", e).into());
            },
        };

        debug!("using adapter {:?}", central);
        debug!("adapter info: {:?}", central.adapter_info().await?);

        // Linux remembers devices that have been paired and can try
        // to connect to them without scanning. Windows needs to scan,
        // and the delay we have may not be long enough for every
        // adapter (but I've tested several and it seems ok).
        // Of course, the Flipper must already be paired.
        
        #[cfg(target_os = "windows")]
        FlipperBle::flipper_scan(&central).await?;
        
        let flip =
            if let Some(d) = Self::find_device_named(flipper_name, &central).await {
                d
            } else {
                return Err(format!("no device with name {:?} found", flipper_name).into());
            };

        flip.connect().await?;
        flip.discover_services().await?;

        info!("connected to Flipper {}", flipper_name);
        
        Ok(FlipperBle {
            proto: ProtobufCodec::new(),
            flipper: flip,
        })
    }

    pub async fn disconnect(&self) -> Result<(), Box<dyn Error>> {
        self.flipper.disconnect().await?;
        Ok(())
    }

    fn get_rx_chr(&self) -> Characteristic {
        let chars = self.flipper.characteristics();
        let rx_chr = chars
            .iter()
            .find(|c| c.uuid == FLIPPER_RX_CHR_UUID)
            .unwrap();

        rx_chr.clone()
    }

    fn get_tx_chr(&self) -> Characteristic {
        let chars = self.flipper.characteristics();
        let tx_chr = chars
            .iter()
            .find(|c| c.uuid == FLIPPER_TX_CHR_UUID)
            .unwrap();

        tx_chr.clone()
    }

    fn get_flow_chr(&self) -> Characteristic {
        let chars = self.flipper.characteristics();
        let flow_chr = chars
            .iter()
            .find(|c| c.uuid == FLIPPER_FLOW_CTRL_CHR_UUID)
            .unwrap();

        flow_chr.clone()
    }
    
    fn make_file_progress_bar(&self, bytes_length: u64) -> ProgressBar {
        let pb = ProgressBar::new(bytes_length);
        pb.set_style(ProgressStyle::with_template(
            "[{wide_bar:.cyan/blue}] {bytes}/{total_bytes} {elapsed}")
                     .unwrap()
                     .progress_chars("#>-"));

        pb
    }

    /// Upload a file to a specific filename on the Flipper over BLE.
    ///
    /// # Arguments
    ///
    /// * `file`: Path to file to upload, this will be opened and read
    ///           by the function.
    /// * `dest`: Full path (i.e. `/ext/apps/GPIO/app.fap`) on Flipper to upload to
    pub async fn upload_file(&mut self, file: &Path, dest: &str) -> Result<(), Box<dyn Error>> {
        // TODO: we can chunk protobuf as much as needed, so we can chunk a packet that has a long path
        if dest.len() > PROTOBUF_CHUNK_SIZE {
            return Err(format!("Destination path too long! Must be shorter than {} characters", PROTOBUF_CHUNK_SIZE).into());
        }

        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();
        let flow_chr = self.get_flow_chr();

        // get filesize for the progress bar
        let filesize = fs::metadata(file)?.len();

        let (file_chunk_sizes, packet_stream) =
            self.proto.create_write_request_packets(file, dest)?;
        debug!("sending {} packets total", packet_stream.len());
        // The Flipper only responds when the has_next flag is false,
        // you can see that in action at
        // https://github.com/flipperdevices/flipperzero-firmware/blob/dev/applications/services/rpc/rpc_storage.c#L473
        // Also, all it's doing is calling storage_file_write(), so
        // really we're depending on a handful of cycles and the SD
        // card.

        // The data is sent correctly but we get warnings (in the
        // Flipper log) like this every few packets:
        // 10560 [W][BtSerialSvc] Received 245, while was ready to receive 37 bytes. Can lead to buffer overflow!
        // Even so, it's fine.
        self.flipper.subscribe(&flow_chr).await?;
        let mut stream = self.flipper.notifications().await?;

        // Progress bar is representative of only the actual bytes in
        // the file, not including the data in the protobuf messages.
        let pb = self.make_file_progress_bar(filesize);

        // This loop waits a small time between packets, but if it
        // gets a notification on the flow control char, it waits a
        // long time. (This seems counterintuitive, because every time
        // we actually get a notification, the available buffer size
        // is the full 1024 bytes. Basically, I don't know why this
        // works, but it does).
        let mut pos: u64 = 0;
        for (p, chunk_size) in packet_stream.iter().zip(file_chunk_sizes.iter()) {
            self.flipper.write(&rx_chr, p, WriteType::WithoutResponse).await?;
            pos += u64::try_from(*chunk_size)?;
            pb.set_position(pos);
            // now_or_never() evaluates and consumes the future
            // immediately, returning an Option with the
            // ValueNotification. We're using it to check if there's a
            // new notification in the stream.

            // Waiting when we get this notification also seems to
            // help (slightly fewer buffer overrun warnings?), but we
            // still get them. Furthermore, it's not good to run with
            // debug-level logging, because it causes a timeout.
            if stream.next().now_or_never().is_some() {
                // (we don't care about the value of the notification)
                
                // The data in this characteristic is the free space
                // left in the BLE serial buffer on the Flipper, as a
                // 32-bit big-endian integer. In this situation, it's
                // always the value 1024, indicating that the buffer
                // is empty. I don't know how you're supposed to use
                // it.
                
                // 800 ms is a good sleep here, it causes it to run
                // slowly enough to not lag notably at the end.
                time::sleep(Duration::from_millis(800)).await;

            }
            time::sleep(Duration::from_millis(80)).await;
        }
        
        pb.finish();
        debug!("sent all packets!");

        time::sleep(Duration::from_millis(400)).await;
        let response = self.flipper.read(&tx_chr).await?;
        let pb_response = ProtobufCodec::parse_response(&response)?;
        debug!("response received: {:?}", pb_response);

        if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::OK.into() {
            Ok(())
        } else {
            Err(format!("Flipper returned error: {:?}", pb_response.1).into())
        }
    }

    // This is the main thing that doesn't work with Intel adapters.
    pub async fn download_file(&mut self, path: &str, dest: &Path) -> Result<(), Box<dyn Error>> {
        if path.len() > PROTOBUF_CHUNK_SIZE {
            return Err(format!("Filename too long! Must be shorter than {} characters", PROTOBUF_CHUNK_SIZE).into());
        }
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        // Getting data back from the Flipper is basically as simple
        // as waiting for indications and checking if it's a full
        // protobuf message.
        self.flipper.subscribe(&tx_chr).await?;

        // Do a stat request so that we can get the size of the file
        let mut stream = self.flipper.notifications().await?;
        let stat_request = self.proto.create_stat_request_packet(path)?;

        debug!("encoded stat request: {:?}", format_u8_slice(&stat_request));

        let mut full_protobuf: Vec<u8> = Vec::new();
        self.flipper.write(&rx_chr, &stat_request, WriteType::WithoutResponse).await?;

        let filesize = loop {
            if let Some(Some(response)) = stream.next().now_or_never() {
                full_protobuf.extend(response.value);
                match ProtobufCodec::parse_response(&full_protobuf) {
                    Ok(m) => {
                        if let Some(flipper_pb::flipper::main::Content::StorageStatResponse(
                            r)) = m.1.content {
                            debug!("received file size: {:?}", r.file.size);
                            break r.file.size;
                        } else if let Some(flipper_pb::flipper::main::Content::Empty(_)) = m.1.content {
                            // Flipper returns Empty { } when the path is bad
                            debug!("received empty response (bad path)");
                            return Err("Invalid Flipper path! Check that the path is correct.".into());
                        } else {
                            error!("received unexpected protobuf response: {:?}", m.1.content);
                            return Err("".into());
                        }
                    },
                    Err(e) => {
                        debug!("protobuf error (incomplete packet): {:?}", e);
                    }
                };
            }
        };

        // now read the contents of the file
        let read_request = self.proto.create_read_request_packet(path)?;
        
        self.flipper.write(&rx_chr, &read_request, WriteType::WithResponse).await?;
        time::sleep(Duration::from_millis(200)).await;
        debug!("wrote read request");
        let pb = self.make_file_progress_bar(u64::try_from(filesize)?);

        let mut file_pos: u64 = 0;
        full_protobuf.clear();
        let mut file_contents = Vec::new();
        // data arrives when we get a notification
        loop {
            if let Some(Some(response)) = stream.next().now_or_never() {
                full_protobuf.extend(response.value);
                // if the protobuf message is complete, do something
                // with it, otherwise just wait for the next message
                match ProtobufCodec::parse_response(&full_protobuf) {
                    Ok(m) => {
                        if let Some(flipper_pb::flipper::main::Content::StorageReadResponse(
                            r)) = m.1.content {
                            file_contents.extend(r.file.data.iter());
                            file_pos += u64::try_from(r.file.data.len())?;
                            pb.set_position(file_pos);
                        }
                        // if we're on the last packet, stop getting data
                        if !m.1.has_next {
                            break;
                        }
                        full_protobuf.clear();
                    },
                    Err(e) => {
                        debug!("protobuf error (incomplete packet): {:?}", e);
                    }
                };
            }
        }
        debug!("all packets received, saving file");

        pb.finish();
        // write out the file
        let mut out = fs::File::create(dest)?;
        out.write_all(&file_contents)?;

        // should we send an OK?
        self.proto.inc_command_id();

        let ok_response = self.proto.create_ok_packet()?;

        self.flipper.write(&rx_chr, &ok_response, WriteType::WithoutResponse).await?;
        debug!("Wrote OK to Flipper");
        Ok(())
    }
    
    /// Launch an app at a path on the Flipper. Filename must be shorter
    /// than PROTOBUF_CHUNK_SIZE.
    ///
    /// # Arguments
    ///
    /// `app`: Flipper path to .fap file to launch
    pub async fn launch(&mut self, app: &str) -> Result<(), Box<dyn Error>> {
        if app.len() > PROTOBUF_CHUNK_SIZE {
            return Err(format!("Path too long! Must be shorter than {} characters", PROTOBUF_CHUNK_SIZE).into());
        }

        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        let launch_packet = self.proto.create_launch_request_packet(app)?;
        debug!("encoded launch request: {:?}", format_u8_slice(&launch_packet));
        self.flipper.write(&rx_chr, &launch_packet, WriteType::WithoutResponse).await?;

        // we're expecting just an Ok or something similarly short, so we don't need the loop
        let response = self.flipper.read(&tx_chr).await?;
        let pb_response = ProtobufCodec::parse_response(&response)?;
        debug!("response received: {:?}", pb_response);

        if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::OK.into() {
            Ok(())
        } else if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::ERROR_INVALID_PARAMETERS.into() {
            Err("Application path is invalid!".into())
        } else {
            Err(format!("Flipper returned unexpected response: {:?}", pb_response).into())
        }
    }

    /// Print directories and files found at a certain path on the
    /// Flipper. Path must be less than PROTOBUF_CHUNK_SIZE.
    ///
    /// # Arguments
    ///
    /// * `path`: Flipper path to get listing from
    pub async fn list(&mut self, path: &str) -> Result<(), Box<dyn Error>> {
        if path.len() > PROTOBUF_CHUNK_SIZE {
            return Err(format!("Path too long! Must be shorter than {} characters", PROTOBUF_CHUNK_SIZE).into());
        }
        
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        // the tx char has attribute indicate, and the Flipper expects
        // the indicate ACK before it will send the next protobuf packet, if has_next is true
        self.flipper.subscribe(&tx_chr).await?;
        let mut stream = self.flipper.notifications().await?;

        // write the list request
        let list_packet = self.proto.create_list_request_packet(path)?;
        debug!("encoded list packet: {:x?}", format_u8_slice(&list_packet));
        self.flipper.write(&rx_chr, &list_packet, WriteType::WithoutResponse).await?;

        let mut entries = Vec::new();

        // wait for data from flipper, receiving as long as the
        // has_next field in the protobuf packet is true
        let mut full_protobuf = Vec::new();
        loop {
            if let Some(Some(response)) = stream.next().now_or_never() {
                full_protobuf.extend(response.value);
                match ProtobufCodec::parse_response(&full_protobuf) {
                    Ok(m) => {
                        if let Some(flipper_pb::flipper::main::Content::StorageListResponse(r)) = m.1.content {
                            for f in r.file {
                                debug!("complete File block: {:?}", f);
                                entries.push(f);
                            }
                            // if we're on the last packet, stop getting data
                            if !m.1.has_next {
                                break;
                            };
                        } else if let Some(flipper_pb::flipper::main::Content::Empty(_)) = m.1.content {
                            debug!("received empty response (bad path)");
                            return Err("Invalid Flipper path! Check that the path is correct.".into());
                        } else {
                            error!("received unexpected protobuf response: {:?}", m.1.content);
                            return Err("".into());
                        }
                        full_protobuf.clear();
                    },
                    Err(e) => {
                        debug!("protobuf error (incomplete packet): {:?}", e);
                    }
                };
            }
        };
        
        // process into dirs and files, and sort by name
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        info!("files at Flipper path {:?}:", path);
        for f in entries {
            if f.type_ == flipper_pb::storage::file::FileType::DIR.into() {
                dirs.push(f);
            } else {
                files.push(f);
            }
        }

        // sort ascending
        dirs.sort_by(|a, b| a.name.cmp(&b.name));
        files.sort_by(|a, b| a.name.cmp(&b.name));

        // dirs don't have size
        for d in dirs {
            println!(" dir:  {:?}", d.name);
        }

        for f in files {
            println!(" file: {:?}, size: {:?}", f.name, f.size);
        }
        
        Ok(())
    }

    /// Play the AV alert on the Flipper to help you find it.
    pub async fn alert(&mut self) -> Result<(), Box<dyn Error>> {
        let rx_chr = self.get_rx_chr();

        let packet = self.proto.create_alert_request_packet()?;
        self.flipper.write(&rx_chr, &packet, WriteType::WithoutResponse).await?;

        Ok(())
    }

    pub async fn sync_datetime(&mut self) -> Result<(), Box<dyn Error>> {
        let rx_chr = self.get_rx_chr();

        let now = chrono::Local::now();
        debug!("using datetime {:?}", now);
        let packet = self.proto.create_set_datetime_request_packet(now.into())?;

        self.flipper.write(&rx_chr, &packet, WriteType::WithoutResponse).await?;

        Ok(())
    }   
}

