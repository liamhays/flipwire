use futures::StreamExt;
use futures::FutureExt;
use btleplug::api::{Central, Manager as _, Peripheral as _, WriteType, Characteristic};
use btleplug::platform::{Manager, Peripheral, Adapter};
use tokio::time;
use tokio::time::Duration;
use uuid::{uuid, Uuid};
use indicatif::{ProgressBar, ProgressStyle};
use chrono::TimeZone;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::error::Error;
use std::convert::TryFrom;

use crate::flipper_pb;
use crate::protobuf_codec::ProtobufCodec;

// Each function follows basically the same principle:
// - Get a protobuf message from protobuf_codec
// - Send its chunks to the Flipper's RX characteristic
// - Wait for a response as necessary.

// the uuid that we write to
const FLIPPER_RX_CHR_UUID: Uuid = uuid!("19ed82ae-ed21-4c9d-4145-228e62fe0000");
// the uuid that we read from
const FLIPPER_TX_CHR_UUID: Uuid = uuid!("19ed82ae-ed21-4c9d-4145-228e61fe0000");
// flow control
const FLIPPER_FLOW_CTRL_CHR_UUID: Uuid = uuid!("19ed82ae-ed21-4c9d-4145-228e63fe0000");
// Delay used for writing chunks of a single command to a
// characteristic. 20 ms seems to work, probably because incomplete
// pieces of a protobuf command sit in memory until they're complete,
// so we're not waiting on storage or anything else until the command
// is fully sent.
const FLIPPER_BLE_PROTOBUF_CHUNK_DELAY: u64 = 20;

/// Representation of a Flipper device connected over Bluetooth LE
pub struct FlipperBle {
    flipper: Peripheral,
    proto: ProtobufCodec,
}

// TODO: Flipper returns ERROR_DECODE when it gets a malformed
// protobuf packet, then ends the RPC session. We need to watch for
// this and take the appropriate actions.

// prints a &[u8] in a style very similar to a Python bytearray
// from https://stackoverflow.com/a/41450295
/*use std::ascii::escape_default;
fn format_u8_slice(bs: &[u8]) -> String {
    let mut visible = String::new();
    for &b in bs {
        let part: Vec<u8> = escape_default(b).collect();
        visible.push_str(std::str::from_utf8(&part).unwrap());
    }
    visible
}
 */

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

        // GapStateAdvLowPower sets a maximum interval of 2.5 seconds,
        // so we wait for 3x that. We used to have 5 seconds but no
        // basis for that.
        // See https://github.com/flipperdevices/flipperzero-firmware/blob/e6f078eeb758992aef0edaf94a23eac846ca8746/targets/f7/ble_glue/gap.c#L375
        time::sleep(Duration::from_millis(3*2500)).await;
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

        // We also can't use the nice async scan notification stream,
        // because it doesn't say anything about device names.
        #[cfg(target_os = "windows")]
        FlipperBle::flipper_scan(&central).await?;

        let flip =
            if let Some(d) = Self::find_device_named(flipper_name, &central).await {
                d
            } else {
                return Err(format!("no device with name {:?} found", flipper_name).into());
            };

        if !flip.is_connected().await? {
            flip.connect().await?;
            info!("connected to Flipper {}", flipper_name);
        } else {
            info!("already connected to Flipper {}", flipper_name);
        }

        flip.discover_services().await?;
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
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();
        let flow_chr = self.get_flow_chr();

        // get filesize for the progress bar
        let filesize = fs::metadata(file)?.len();
        let file_contents = fs::read(file)?;

        let write_request_chunks =
            self.proto.create_write_request_packets(&file_contents, dest)?;
        debug!("sending {} packets total", write_request_chunks.len());
        // The Flipper only responds when the has_next flag is false,
        // you can see that in action at
        // https://github.com/flipperdevices/flipperzero-firmware/blob/dev/applications/services/rpc/rpc_storage.c#L473
        // Also, all it's doing is calling storage_file_write(), so
        // really we're depending on a handful of cycles and the SD
        // card.

        // The data is sent correctly but we get warnings (in the
        // Flipper log) like this every few packets:
        // `10560 [W][BtSerialSvc] Received 245, while was ready to receive 37 bytes. Can lead to buffer overflow!`
        // I don't like that it does this but I don't know how to fix it.

        // Furthermore (there are notes on this in protobuf_codec.rs),
        // uploads are slower than the mobile app. I don't know why
        // this is, because the mobile app also doesn't cause the
        // overrun warnings.
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
        for p in write_request_chunks {
            // Write one chunk, which will be a couple of
            // packets. These are continuous pieces of a single
            // protobuf message, so we don't wait for a response
            // because there won't be one.
            for v in p.packets {
                self.flipper.write(&rx_chr, &v, WriteType::WithoutResponse).await?;
                time::sleep(Duration::from_millis(FLIPPER_BLE_PROTOBUF_CHUNK_DELAY)).await;
            }
            pos += u64::try_from(p.file_byte_count)?;
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
                // is empty.
                
                // 800 ms is a good sleep here. Sometimes we end up in
                // this state many times during a transfer, so keeping
                // this short is desirable.
                time::sleep(Duration::from_millis(800)).await;

            }
            // On Linux at least (with my goofy Intel 7265), 140 ms
            // works very well and stops the host from timing out
            // waiting for a reply after sending the whole file, a
            // problem that happens most often right after the adapter
            // has been enabled.
            time::sleep(Duration::from_millis(140)).await;
        }
        
        pb.finish();
        debug!("sent all packets!");

        // This is the place where the ATT error occurs. It might be
        // another Stone Peak oddity, but sometimes the upload
        // finishes but this step fails with an error about ATT
        // 0x0b. 0x0b is a Read Response opcode, maybe it's something
        // with the delay?
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

    // This is the main thing that doesn't work with Intel Stone Peak adapters.
    pub async fn download_file(&mut self, path: &str, dest: &Path) -> Result<(), Box<dyn Error>> {
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        // Getting data back from the Flipper is basically as simple
        // as waiting for indications and checking if it's a full
        // protobuf message.
        self.flipper.subscribe(&tx_chr).await?;

        // Do a stat request so that we can get the size of the file
        let mut stream = self.flipper.notifications().await?;
        let stat_request = self.proto.create_stat_request_packet(path)?;

        for chunk in stat_request {
            self.flipper.write(&rx_chr, &chunk, WriteType::WithoutResponse).await?;
            time::sleep(Duration::from_millis(FLIPPER_BLE_PROTOBUF_CHUNK_DELAY)).await;
        }

        let mut full_protobuf: Vec<u8> = Vec::new();

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
        
        for chunk in read_request {
            self.flipper.write(&rx_chr, &chunk, WriteType::WithoutResponse).await?;
            time::sleep(Duration::from_millis(FLIPPER_BLE_PROTOBUF_CHUNK_DELAY)).await;
        }

        time::sleep(Duration::from_millis(200)).await;
        debug!("wrote read request");
        let pb = self.make_file_progress_bar(From::from(filesize));

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

    /// Delete a file at a path on the Flipper. Filename must be shorter than PROTOBUF_CHUNK_SIZE.
    ///
    /// # Arguments
    ///
    /// `path`: Flipper path to file to delete
    /// `recursive`: Delete recursively if true
    pub async fn delete_file(&mut self, path: &str, recursive: bool) -> Result<(), Box<dyn Error>> {
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        let delete_packet = self.proto.create_delete_request_packet(path, recursive)?;
        for chunk in delete_packet {
            self.flipper.write(&rx_chr, &chunk, WriteType::WithoutResponse).await?;
            time::sleep(Duration::from_millis(FLIPPER_BLE_PROTOBUF_CHUNK_DELAY)).await;
        }

        let response = self.flipper.read(&tx_chr).await?;
        let pb_response = ProtobufCodec::parse_response(&response)?;
        debug!("response received: {:?}", pb_response);

        // If the file doesn't exist, Flipper explicitly returns
        // CommandStatus OK. See
        // https://github.com/flipperdevices/flipperzero-firmware/blob/dev/applications/services/rpc/rpc_storage.c#L550
        if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::OK.into() {
            Ok(())
        } else if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::ERROR_STORAGE_INVALID_NAME.into() {
            Err("Invalid name specified!".into())
        } else {
            Err(format!("Flipper returned unexpected response: {:?}", pb_response).into())
        }
    }
    
    /// Launch an app at a path on the Flipper. Filename must be shorter
    /// than PROTOBUF_CHUNK_SIZE.
    ///
    /// # Arguments
    ///
    /// `app`: Flipper path to .fap file to launch
    /// `args`: Arguments to the app, can be blank
    pub async fn launch(&mut self, app: &str, args: &str) -> Result<(), Box<dyn Error>> {
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        let launch_packet = self.proto.create_launch_request_packet(app, args)?;
        for chunk in launch_packet {
            self.flipper.write(&rx_chr, &chunk, WriteType::WithoutResponse).await?;
            time::sleep(Duration::from_millis(20)).await;
        }

        // we're expecting just an Ok or something similarly short, so we don't need the loop
        let response = self.flipper.read(&tx_chr).await?;
        let pb_response = ProtobufCodec::parse_response(&response)?;
        debug!("response received: {:?}", pb_response);

        // If you try to load a nonexistent file in an app, the app is
        // the one that displays an error. No error is relayed back
        // over RPC.
        if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::OK.into() {
            Ok(())
        } else if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::ERROR_INVALID_PARAMETERS.into() {
            Err("Application path is invalid!".into())
        } else if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::ERROR_APP_CANT_START.into() {
            Err("App can't start! Did you specify the path to a Flipper app and is the app up to date?".into())
        } else if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::ERROR_APP_SYSTEM_LOCKED.into() {
            Err("Another app is already running! Close it and try again.".into())
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
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        // the tx char has attribute indicate, and the Flipper expects
        // the indicate ACK before it will send the next protobuf packet, if has_next is true
        self.flipper.subscribe(&tx_chr).await?;
        let mut stream = self.flipper.notifications().await?;

        // write the list request
        let list_packet = self.proto.create_list_request_packet(path)?;
        for chunk in list_packet {
            self.flipper.write(&rx_chr, &chunk, WriteType::WithoutResponse).await?;
            // 20 ms seems to work, this is all going into Flipper
            // memory anyway so it's quick
            time::sleep(Duration::from_millis(20)).await;
        }

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

        // only one chunk
        let packet = self.proto.create_alert_request_packet()?;
        self.flipper.write(&rx_chr, &packet, WriteType::WithoutResponse).await?;

        Ok(())
    }

    /// Sync the Flipper's date and time to the computer's date and time.
    pub async fn sync_datetime(&mut self) -> Result<(), Box<dyn Error>> {
        // things in this function are a little out of order for
        // Flipper time accuracy, even if it doesn't really matter
        let rx_chr = self.get_rx_chr();
        let tx_chr = self.get_tx_chr();

        // no chunking here
        let request = self.proto.create_get_datetime_request_packet()?;
        self.flipper.write(&rx_chr, &request, WriteType::WithoutResponse).await?;
        let mut now = chrono::Local::now();
        // only one packet comes in response
        let response = self.flipper.read(&tx_chr).await?;

        match ProtobufCodec::parse_response(&response) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::SystemGetDatetimeResponse(r)) = m.1.content {
                    // calculate time skew
                    let flipper_time = chrono::Local.with_ymd_and_hms(
                        r.datetime.year as i32,
                        r.datetime.month,
                        r.datetime.day,
                        r.datetime.hour,
                        r.datetime.minute,
                        r.datetime.second,
                    ).unwrap();

                    info!("Flipper time skew in ms: {:?}", (now - flipper_time).num_milliseconds());
                } else {
                    error!("received unexpected protobuf response: {:?}", m.1.content);
                    return Err("".into());
                }
            },
            Err(e) => {
                error!("protobuf error: {:?}", e);
            },
        };

        // recalculate time for update
        now = chrono::Local::now();
        let packet = self.proto.create_set_datetime_request_packet(now.into())?;
        self.flipper.write(&rx_chr, &packet, WriteType::WithoutResponse).await?;

        debug!("using datetime {:?}", now);
        
        Ok(())
    }

}

