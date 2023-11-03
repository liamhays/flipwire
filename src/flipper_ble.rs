use futures::StreamExt;
use futures::FutureExt;
use btleplug::api::{Central, Manager as _, Peripheral as _, WriteType};
use btleplug::platform::{Manager, Peripheral, Adapter};
use tokio;
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

// Known working adapters:
// - Cambridge Silicon BLE dongle
// - AzureWave AW-CM256SM (based on CYW43455, found on the Quartz64 Model B)
// - 
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
use std::ascii::escape_default;
fn show(bs: &[u8]) -> String {
    let mut visible = String::new();
    for &b in bs {
        let part: Vec<u8> = escape_default(b).collect();
        visible.push_str(std::str::from_utf8(&part).unwrap());
    }
    visible
}

// TODO: flipper doesn't like paths with trailing slashes
impl FlipperBle {
    async fn find_device_named(flipper_name: &str, central: &Adapter) -> Option<Peripheral> {
        for p in central.peripherals().await.unwrap() {
            if p.properties()
                .await
                .unwrap()
                .unwrap()
                .local_name
                .iter()
                .any(|name| name.contains(flipper_name)) {
                    info!("paired Flipper device found: {:?}", p);
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
                if adapters.len() < 1 {
                    return Err(format!("no Bluetooth adapters found").into());
                }
                adapters.into_iter().nth(0).unwrap()
            },
            Err(e) => {
                return Err(format!("error finding Bluetooth adapters: {:?}", e).into());
            },
        };
        
        debug!("Using adapter {:?}", central);
        // The Flipper must be paired already 
        let flip =
            if let Some(d) = Self::find_device_named(flipper_name, &central).await {
                d
            } else {
                return Err(format!("no paired device with name {:?} found", flipper_name).into());
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

    fn get_file_progress_bar(&self, bytes_length: u64) -> ProgressBar {
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
        
        debug!("Uploading file {:?} to {:?}", file, dest);
        // unless we wrestle with static lifetimes, each function has
        // to get the characteristics
        let chars = self.flipper.characteristics();
        let rx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_RX_CHR_UUID)
            .unwrap();
        let tx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_TX_CHR_UUID)
            .unwrap();
        let flow_chr =
            &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_FLOW_CTRL_CHR_UUID)
            .unwrap();
        // Send all packets to the Flipper. The sleep isn't really
        // necessary (I think there's some kind of negotiation or
        // buffering), but since the Flipper doesn't ACK each packet,
        // it seems wise to have a delay.
        let packet_stream = self.proto.create_storage_write_packets(file, dest)?;
        debug!("{} packets total", packet_stream.len());
        info!("sending file {:?}", file);
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

        // TODO: format as bytes
        let mut total_bytes = 0;
        for i in &packet_stream {
            total_bytes += i.len();
        }
        // TODO: change this to only bytes in the file
        let pb = self.get_file_progress_bar(u64::try_from(total_bytes)?);

        // This loop waits a small time between packets, or if it gets
        // a notification on the flow control char, it waits a long
        // time. (This seems counterintuitive, because every time we
        // actually get a notification, the available buffer size is
        // the full 1024 bytes. Basically, I don't know why this
        // works, but it does).
        let mut pos: u64 = 0;
        for p in packet_stream {
            self.flipper.write(&rx_chr, &p, WriteType::WithoutResponse).await?;
            pos += u64::try_from(p.len())?;
            pb.set_position(pos);
            // now_or_never() evaluates and consumes the future
            // immediately, returning an Option with the
            // ValueNotification. This approach to async subscription
            // works.

            // Waiting when we get this notification also seems to
            // help (slightly fewer buffer overrun warnings?), but we
            // still get them. Furthermore, it's not good to run with
            // debug-level logging, because it causes a timeout.
            if let Some(Some(_)) = stream.next().now_or_never() {
                // The data in this characteristic is the free space
                // left in the BLE serial buffer on the Flipper, as a
                // 32-bit big-endian integer.
                
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
            Err(format!("Flipper returned error: {:?}", response[1]).into())
        }
    }

    pub async fn download_file(&mut self, path: &str, dest: &Path) -> Result<(), Box<dyn Error>> {
        if path.len() > PROTOBUF_CHUNK_SIZE {
            return Err(format!("Filename too long! Must be shorter than {} characters", PROTOBUF_CHUNK_SIZE).into());
        }
        debug!("Requesting data of file {:?}", path);
        
        let chars = self.flipper.characteristics();
        let rx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_RX_CHR_UUID)
            .unwrap();
        let tx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_TX_CHR_UUID)
            .unwrap();
        // Protobuf messages are split up, so we have to accumulate
        // them until we get a complete message. The Flipper correctly
        // chunks data and sends it through the TX characteristic. 
        
        // The Flipper source code (see
        // firmware/target/f7/ble_glue/services/serial_service.c)
        // writes data to the tx char with and without indicate
        // enabled. I think this may be a way to work around either
        // MTU, core 2 bugs, or some other BLE thing. We only need to
        // worry about when the Flipper writes with indicate.

        // All of the above explains why we use this "loop {}"
        // approach below.
        
        // We have to do a StorageStatRequest, because
        // StorageListRequest is only for directories, and because the
        // file.size field in the StorageReadResponse is 0

        // The data we are sending is correct, it matches what we see
        // from the app. Sending a StatRequest before the ReadRequest
        // doesn't really help.
        
        self.flipper.subscribe(&tx_chr).await?;

        let mut stream = self.flipper.notifications().await?;
        /*

        let dat2 = vec![0x0f, 0x08, 0xe0, 0x04, 0x3a, 0x0a, 0x0a, 0x08, 0x2f, 0x65, 0x78, 0x74, 0x2f, 0x6e, 0x66, 0x63];
        // an actual read request!
        let dat3 = vec![0x23, 0x08, 0xe1, 0x04, 0x4a, 0x1e, 0x0a, 0x1c, 0x2f, 0x65, 0x78, 0x74, 0x2f, 0x6e, 0x66, 0x63, 0x2f, 0x53, 0x68, 0x61, 0x64, 0x65, 0x73, 0x5f, 0x6f, 0x66, 0x5f, 0x67, 0x72, 0x65, 0x65, 0x6e, 0x2e, 0x6e, 0x66, 0x63];

        
        println!("data 3: {:?}", ProtobufCodec::parse_response(&dat3));
         */
        /*let dat2 = vec![                
/* Reassembled BTHCI ACL (418 bytes) */
0x8e, /* .....#.. */
0x04, 0x08, 0xe1, 0x04, 0x18, 0x01, 0x52, 0x86, /* ......R. */
0x04, 0x0a, 0x83, 0x04, 0x22, 0x80, 0x04, 0x43, /* ...."..C */
0x20, 0x33, 0x31, 0x0a, 0x42, 0x6c, 0x6f, 0x63, /*  31.Bloc */
0x6b, 0x20, 0x31, 0x32, 0x3a, 0x20, 0x30, 0x30, /* k 12: 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x0a, 0x42, 0x6c, /* 00 00.Bl */
0x6f, 0x63, 0x6b, 0x20, 0x31, 0x33, 0x3a, 0x20, /* ock 13:  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x0a, /* 0 00 00. */
0x42, 0x6c, 0x6f, 0x63, 0x6b, 0x20, 0x31, 0x34, /* Block 14 */
0x3a, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* : 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x0a, 0x42, 0x6c, 0x6f, 0x63, 0x6b, 0x20, /* 0.Block  */
0x31, 0x35, 0x3a, 0x20, 0x46, 0x46, 0x20, 0x46, /* 15: FF F */
0x46, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, 0x20, /* F FF FF  */
0x46, 0x46, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, /* FF FF FF */
0x20, 0x30, 0x37, 0x20, 0x38, 0x30, 0x20, 0x36, /*  07 80 6 */
0x39, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, 0x20, /* 9 FF FF  */
0x46, 0x46, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, /* FF FF FF */
0x20, 0x46, 0x46, 0x0a, 0x42, 0x6c, 0x6f, 0x63, /*  FF.Bloc */
0x6b, 0x20, 0x31, 0x36, 0x3a, 0x20, 0x30, 0x30, /* k 16: 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
    0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
    0x30, 0x30, 0x20, 0x30, 0x30, 0x0a, 0x42, 0x6c, /* 00 00.Bl */
0x6f, 0x63, 0x6b, 0x20, 0x31, 0x37, 0x3a, 0x20, /* ock 17:  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x0a, /* 0 00 00. */
0x42, 0x6c, 0x6f, 0x63, 0x6b, 0x20, 0x31, 0x38, /* Block 18 */
0x3a, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* : 00 00  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, /*  00 00 0 */
0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x20, /* 0 00 00  */
            0x30, 0x30,                                      /* 00 */
                 0x20, /* x....#.  */
0x30, 0x30, 0x20, 0x30, 0x30, 0x20, 0x30, 0x30, /* 00 00 00 */
0x20, 0x30, 0x30, 0x20, 0x30, 0x30, 0x0a, 0x42, /*  00 00.B */
0x6c, 0x6f, 0x63, 0x6b, 0x20, 0x31, 0x39, 0x3a, /* lock 19: */
0x20, 0x46, 0x46, 0x20, 0x46, 0x46, 0x20, 0x46, /*  FF FF F */
0x46, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, 0x20, /* F FF FF  */
0x46, 0x46, 0x20, 0x46, 0x46, 0x20, 0x30, 0x37, /* FF FF 07 */
0x20, 0x38, 0x30, 0x20, 0x36, 0x39, 0x20, 0x46, /*  80 69 F */
0x46, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, 0x20, /* F FF FF  */
0x46, 0x46, 0x20, 0x46, 0x46, 0x20, 0x46, 0x46, /* FF FF FF */
0x0a, 0x42, 0x6c, 0x6f, 0x63, 0x6b, 0x20, 0x32, /* .Block 2 */
0x30, 0x3a, 0x20, 0x33, 0x31, 0x20, 0x33, 0x31, /* 0: 31 31 */
0x20, 0x33, 0x34, 0x20, 0x33, 0x38, 0x20, 0x34, /*  34 38 4 */
0x34, 0x20, 0x33, 0x31, 0x20, 0x33, 0x34, 0x20, /* 4 31 34  */
0x33, 0x30, 0x20, 0x33, 0x35, 0x20, 0x33, 0x33, /* 30 35 33 */
0x20, 0x33, 0x30, 0x20                          /*  30  */

        ];*/
        //println!("data 2: {:?}", ProtobufCodec::parse_response(&dat2));
        let stat_request = self.proto.create_storage_stat_packet(&path)?;
        
        println!("stat request: {:x?}", stat_request);
        println!("{}", show(&stat_request));

        debug!("Writing request to Flipper");
        let mut full_protobuf: Vec<u8> = Vec::new();
        self.flipper.write(&rx_chr, &stat_request, WriteType::WithoutResponse).await?;

        let filesize = loop {
            if let Some(Some(response)) = stream.next().now_or_never() {
                full_protobuf.extend(response.value);
                match ProtobufCodec::parse_response(&full_protobuf) {
                    Ok(m) => {
                        if let Some(flipper_pb::flipper::main::Content::StorageStatResponse(
                            r)) = m.1.content {
                            debug!("file size: {:?}", r.file.size);
                            break r.file.size;
                        }
                    },
                    Err(e) => {
                        debug!("protobuf error (incomplete packet): {:?}", e);
                    }
                };
            }
        };

        // this actually seems to help...?
        //println!("sleeping");
        //time::sleep(Duration::from_millis(4000)).await;
        let read_request = self.proto.create_storage_read_packet(path)?;
        println!("{:?}", read_request);
        
        self.flipper.write(&rx_chr, &read_request, WriteType::WithResponse).await?;
        time::sleep(Duration::from_millis(2000)).await;
        debug!("wrote read request");
        //let pb = self.get_file_progress_bar(u64::try_from(filesize)?);

        // Phone gets chunks of 411 bytes, then 117 bytes. we get chunks of 411 bytes, then 116.
        // Connection parameters: Connection Interval: 36 (45 ms), Slave Latency: 0, Supervision Timeout: 42
        let mut file_pos: u64 = 0;
        full_protobuf.clear();
        let mut file_contents = Vec::new();
        // data arrives when we get a notification
        loop {

            if let Some(Some(response)) = stream.next().now_or_never() {
                //debug!("notification: {:?}", response);
                debug!("data len: {:?}", response.value.len());
                full_protobuf.extend(response.value);
                // if the protobuf message is complete, do something
                // with it, otherwise just wait for the next message
                match ProtobufCodec::parse_response(&full_protobuf) {
                    Ok(m) => {
                        if let Some(flipper_pb::flipper::main::Content::StorageReadResponse(
                            r)) = m.1.content {
                            file_contents.extend(r.file.data.iter());
                            //file_pos += u64::try_from(r.file.data.len())?;
                            //pb.set_position(file_pos);
                        }
                        // if we're on the last packet, stop getting data
                        if m.1.has_next == false {
                            break;
                        }
                        info!("good data, clearing vec");
                        full_protobuf.clear();
                        //time::sleep(Duration::from_millis(100)).await;
                    },
                    Err(e) => {
                        debug!("protobuf error (incomplete packet): {:?}", e);
                    }
                };

            }
                
        }
        debug!("outside loop");

        // Acks are being sent, we can see this with `log trace` on the Flipper.
        

        //pb.finish();
        // write out the file
        let mut out = fs::File::create(dest)?;
        out.write_all(&file_contents)?;

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
        debug!("Launching app {:?}", app);
        
        let chars = self.flipper.characteristics();
        let rx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_RX_CHR_UUID)
            .unwrap();
        let tx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_TX_CHR_UUID)
            .unwrap();

        let launch_packet = self.proto.create_launch_packet(app)?;
        self.flipper.write(&rx_chr, &launch_packet, WriteType::WithoutResponse).await?;

        let response = self.flipper.read(&tx_chr).await?;
        let pb_response = ProtobufCodec::parse_response(&response)?;
        debug!("response received: {:?}", pb_response);

        if pb_response.1.command_status == flipper_pb::flipper::CommandStatus::OK.into() {
            Ok(())
        } else {
            Err(format!("Flipper returned error: {:?}", response[1]).into())
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
        
        debug!("Listing path {:?}", path);
        let chars = self.flipper.characteristics();
        let rx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_RX_CHR_UUID)
            .unwrap();
        let tx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_TX_CHR_UUID)
            .unwrap();

        // the tx char has attribute indicate, and the Flipper expects
        // the indicate ACK before it will send the next protobuf packet, if has_next is true
        self.flipper.subscribe(&tx_chr).await?;
        let mut stream = self.flipper.notifications().await?;

        // write the list request
        let list_packet = self.proto.create_list_packet(path)?;
        self.flipper.write(&rx_chr, &list_packet, WriteType::WithoutResponse).await?;

        let mut entries = Vec::new();

        // wait for data from flipper, receiving as long as the
        // has_next field in the protobuf packet is true
        loop {
            if let Some(Some(response)) = stream.next().now_or_never() {
                let pb_response = ProtobufCodec::parse_response(&response.value)?;

                if pb_response.1.command_status != flipper_pb::flipper::CommandStatus::OK.into() {
                    return Err("Flipper returned non-OK protobuf packet".into());
                }
                
                // we're basically coercing the type here because Main can have any type of content
                if let Some(flipper_pb::flipper::main::Content::StorageListResponse(r)) = pb_response.1.content {
                    for f in r.file {
                        debug!("complete File block: {:?}", f);
                        entries.push(f);
                    }
                    // if we're on the last packet, stop getting data
                    if pb_response.1.has_next == false {
                        break;
                    };
                }
            }
        }
            // process into dirs and files, and sort by name
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        info!("Flipper files at {:?}:", path);
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

    pub async fn alert(&mut self) -> Result<(), Box<dyn Error>> {
        let chars = self.flipper.characteristics();
        let rx_chr = &chars
            .iter()
            .find(|c| c.uuid == FLIPPER_RX_CHR_UUID)
            .unwrap();

        let packet = self.proto.create_av_alert_packet()?;
        self.flipper.write(&rx_chr, &packet, WriteType::WithoutResponse).await?;

        Ok(())
    }
    
}

