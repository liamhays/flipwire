use std::error::Error;
use std::path::Path;
use std::fs;

use protobuf::{Message, MessageField, CodedInputStream};
use chrono::Datelike;
use chrono::Timelike;

use crate::flipper_pb;

// The flipperzero_protobuf_py example uses a chunk size of 512, which
// absolutely doesn't work for us, because you can only write up to
// 512 bytes to a characteristic! To leave room for protobuf data, we
// cut that down to 300 bytes.
//
// Furthermore, the Flipper needs a small size so that the serial
// service can correctly process it.
pub const PROTOBUF_CHUNK_SIZE: usize = 350;

// command_id is uint32 in protobuf definition
// I don't have a great name for this struct
pub struct ProtobufCodec {
    command_id: u32,
}
#[allow(dead_code)]
impl ProtobufCodec {
    pub fn new() -> ProtobufCodec {
        ProtobufCodec {
            command_id: 0
        }
    }

    fn new_blank_packet(&mut self, increment: bool) -> flipper_pb::flipper::Main {
        let final_msg = flipper_pb::flipper::Main {
            command_id: self.command_id,
            command_status: flipper_pb::flipper::CommandStatus::OK.into(),
            // give default values to the rest of the fields
            ..Default::default()
        };

        if increment {
            self.command_id += 1;
        }

        final_msg
    }

    /// Increment the global command ID. Call this after the Flipper
    /// sends a successful command.
    pub fn inc_command_id(&mut self) {
        self.command_id += 1;
    }

    /// Returns a Vec<u8> containing an encoded Empty packet with
    /// command status OK, used for responses to the Flipper after an
    /// operation.
    pub fn create_ok_packet(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut final_vec = Vec::new();
        
        let packet = self.new_blank_packet(true);

        packet.write_length_delimited_to_vec(&mut final_vec)?;

        Ok(final_vec)
    }
    /// Returns a Vec<u8> containing an encoded AppStartRequest
    /// protobuf packet to send to the Flipper, or an error if
    /// encoding failed (very unlikely). The app path (or name if it's
    /// a builtin app) must be less than PROTOBUF_CHUNK_SIZE.
    ///
    /// # Arguments
    ///
    /// * `path`: Full Flipper path or builtin app name to launch.
    pub fn create_launch_request_packet(&mut self, path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let launch_request = flipper_pb::application::StartRequest {
            // builtin apps can be launched by name, external ones need a full path
            name: path.to_string(),
            ..Default::default()
        };

        let mut final_msg = self.new_blank_packet(true);
        final_msg.content = Some(flipper_pb::flipper::main::Content::AppStartRequest(launch_request));

        // append the length to the start of the packet
        let mut final_vec = Vec::new();
        
        // this function will encode the length of the packet as a
        // varint at the start of the Vec
        final_msg.write_length_delimited_to_vec(&mut final_vec)?;

        Ok(final_vec)
    }

    /// Returns a Vec<u8> containing an encoded StorageListRequest
    /// protobuf packet for a specific path to send to the Flipper, or
    /// an error if encoding failed. The path must be less than PROTOBUF_CHUNK_SIZE.
    pub fn create_list_request_packet(&mut self, path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let list_request = flipper_pb::storage::ListRequest {
            path: path.to_string(),
            ..Default::default()
        };

        let mut final_msg = self.new_blank_packet(true);
        final_msg.content = Some(flipper_pb::flipper::main::Content::StorageListRequest(list_request));

        let mut final_vec = Vec::new();
        final_msg.write_length_delimited_to_vec(&mut final_vec)?;

        Ok(final_vec)
    }
    
    /// Returns a Vec<Vec<u8>> of encoded StorageWriteRequest packets
    /// containing the content of the file at `file` and the
    /// destination Flipper path `destpath`, or an error if file
    /// reading or encoding occurred.
    ///
    /// # Arguments
    ///
    /// * `file`: Input file to send to the Flipper
    /// * `destpath`: Destination Flipper path, must be a complete path including filename
    ///
    /// # Returns
    ///
    /// * Vec<usize> of the number of _file bytes_ in the corresponding packet
    /// * Vec<Vec<u8>> of packets
    /// This is janky and should probably be changed.
    pub fn create_write_request_packets(
        &mut self,
        file: &Path,
        destpath: &str) -> Result<(Vec<usize>, Vec<Vec<u8>>), Box<dyn Error>> {
        
        let file_contents = fs::read(file)?;
        let mut packet_stream = Vec::new();

        let mut chunk_sizes = Vec::new();

        // Workaround: an empty file will cause the loop to never
        // run. There's no easy "always iterate at least once" wrapper
        // for an iterator, so we do this instead.
        if file_contents.is_empty() {
            debug!("creating packets for empty file");
            let write_request = flipper_pb::storage::WriteRequest {
                path: destpath.to_string(),

                ..Default::default()
            };
            // no data to write here
            
            let mut packet = self.new_blank_packet(false);
            packet.content = Some(flipper_pb::flipper::main::Content::StorageWriteRequest(write_request));
            packet.has_next = false;

            let mut packet_vec = Vec::new();
            packet.write_length_delimited_to_vec(&mut packet_vec)?;
            packet_stream.push(packet_vec);

            // chunk size is 0
            chunk_sizes.push(0);
        } else {
            // Every packet is the same, a WriteRequest, and the Flipper knows
            // if we have more data to send via the has_next flag.
            for index in (0..file_contents.len()).step_by(PROTOBUF_CHUNK_SIZE) {
                let chunk = if index + PROTOBUF_CHUNK_SIZE < file_contents.len() {
                    &file_contents[index..index+PROTOBUF_CHUNK_SIZE]
                } else {
                    &file_contents[index..]
                };
                
                // make a write request packet
                let mut write_request = flipper_pb::storage::WriteRequest {
                    path: destpath.to_string(),

                    ..Default::default()
                };
                // You have to use MessageField::some() to write to the `file` field.
                // There are other fields in the File struct but we don't
                // need to worry about them.
                let mut f = flipper_pb::storage::File::new();
                f.data = chunk.to_vec();
                write_request.file = MessageField::some(f);

                // only increment the packet when we finish the full command
                let mut packet = self.new_blank_packet(false);
                packet.content = Some(flipper_pb::flipper::main::Content::StorageWriteRequest(write_request));
                
                if index + PROTOBUF_CHUNK_SIZE < file_contents.len() {
                    // has_next = true because we still have more data
                    packet.has_next = true;
                } else {
                    packet.has_next = false;
                }
                
                let mut packet_vec = Vec::new();
                packet.write_length_delimited_to_vec(&mut packet_vec)?;
                packet_stream.push(packet_vec);

                chunk_sizes.push(chunk.len());
            }
        }
        // The command ID only increments after every complete
        // command. The packet stream is a series of protobuf commands
        // that represent a single command, so we increment it after
        // we make all the packets.
        self.command_id += 1;
        Ok((chunk_sizes, packet_stream))
    }

    pub fn create_read_request_packet(&mut self, path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let read_request = flipper_pb::storage::ReadRequest {
            path: path.to_string(),

            ..Default::default()
        };

        let mut final_msg = self.new_blank_packet(true);
        final_msg.content = Some(flipper_pb::flipper::main::Content::StorageReadRequest(read_request));
        debug!("read request: {:?}", final_msg);
        let mut final_vec = Vec::new();
        
        final_msg.write_length_delimited_to_vec(&mut final_vec)?;

        Ok(final_vec)
    }

    pub fn create_stat_request_packet(&mut self, path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let stat_request = flipper_pb::storage::StatRequest {
            path: path.to_string(),

            ..Default::default()
        };

        let mut final_msg = self.new_blank_packet(true);
        final_msg.content = Some(flipper_pb::flipper::main::Content::StorageStatRequest(stat_request));
        debug!("stat request: {:?}", final_msg);
        let mut final_vec = Vec::new();
        
        final_msg.write_length_delimited_to_vec(&mut final_vec)?;

        Ok(final_vec)
    }
    pub fn create_alert_request_packet(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut final_msg = self.new_blank_packet(true);
        // we can combine this because PlayAudiovisualAlertRequest has no fields
        final_msg.content = Some(
            flipper_pb::flipper::main::Content::SystemPlayAudiovisualAlertRequest(
                flipper_pb::system::PlayAudiovisualAlertRequest::default()));

        let mut final_vec = Vec::new();
        final_msg.write_length_delimited_to_vec(&mut final_vec)?;

        Ok(final_vec)
    }

    pub fn create_set_datetime_request_packet(&mut self, datetime: chrono::DateTime<chrono::FixedOffset>) -> Result<Vec<u8>, Box<dyn Error>> {

        // SetDatetimeRequest is a thin wrapper around
        // FuriHalRtcDateTime which itself is a thin wrapper around
        // the STM32 LL RTC driver. This driver defines Monday as day
        // 1 and Sunday as day 7, hence number_from_monday() below.
        let datetime_pb = flipper_pb::system::DateTime {
            hour: datetime.hour(),
            minute: datetime.minute(),
            second: datetime.second(),

            day: datetime.day(),
            month: datetime.month(),
            year: datetime.year() as u32,

            weekday: datetime.weekday().number_from_monday(),

            ..Default::default()
        };

        let set_datetime_request = flipper_pb::system::SetDateTimeRequest {
            datetime: MessageField::some(datetime_pb),

            ..Default::default()
        };
        
        let mut final_msg = self.new_blank_packet(true);
        final_msg.content = Some(
            flipper_pb::flipper::main::Content::SystemSetDatetimeRequest(
                set_datetime_request));

        let mut final_vec = Vec::new();
        final_msg.write_length_delimited_to_vec(&mut final_vec)?;
        
        Ok(final_vec)
    }
    
    pub fn parse_response(data: &[u8]) -> Result<(u32, flipper_pb::flipper::Main), Box<dyn Error>> {
        let mut stream = CodedInputStream::from_bytes(data);
        let length = stream.read_raw_varint32()?;
        let s = flipper_pb::flipper::Main::parse_from_reader(&mut stream)?;
        Ok((length, s))
    }
}
