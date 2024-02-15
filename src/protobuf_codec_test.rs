mod protobuf_codec; // make accessible

// Function (unit?) tests! These are in a separate file to keep
// protobuf_codec.rs from getting too big.

use protobuf_codec::ProtobufCodec;

#[cfg(test)]
mod tests {
    #[test]
    fn protobuf_codec_launch_request_test() {
        // check that data can be loaded in and out, from protobuf form to byte data
        let mut p = ProtobufCodec::new();
        // include command id increment in all tests
        p.inc_command_id();
        let path = "/ext/app.fap";
        let args = "/ext/chungus_app.fap";
        let mut launch_chunks = p.create_launch_request_packet(path, args).unwrap();
        
        let mut launch_packet = Vec::new();
        launch_chunks.iter_mut().for_each(|x| launch_packet.append(&mut *x));
        
        match ProtobufCodec::parse_response(&launch_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::AppStartRequest(r)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                    assert_eq!(path, r.name);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }

    #[test]
    fn protobuf_codec_list_packet_test() {
        let mut p = ProtobufCodec::new();
        let path = "/ext/apps";
        p.inc_command_id();
        let mut list_chunks = p.create_list_request_packet(path).unwrap();

        let mut list_packet = Vec::new();
        list_chunks.iter_mut().for_each(|x| list_packet.append(&mut *x));
            
        match ProtobufCodec::parse_response(&list_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::StorageListRequest(r)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                    assert_eq!(path, r.path);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
        
    }

    #[test]
    fn protobuf_codec_write_request_test() {
        // generate some data, package it up, then check to see if the
        // data chunks match the original data
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        
        let mut data = Vec::new();
        for i in 0..1023 {
            data.push(i as u8);
        }

        let write_request_chunks =
            p.create_write_request_packets(&data, "/ext/data.dat").unwrap();

        let mut index = 0;
        for mut chunk in write_request_chunks {
            // This test function takes a different approach than the
            // others. We stitch up the separate Vecs for each
            // ProtobufWriteRequestChunk and pass that to the parser,
            // so that we can get all the data in that chunk at once
            // but still keep file_byte_count available.
            let mut stitched_vec = Vec::new();
            chunk.packets.iter_mut()
                .for_each(|x| stitched_vec.append(x));
            
            match ProtobufCodec::parse_response(&stitched_vec) {
                Ok(m) => {
                    if let Some(flipper_pb::flipper::main::Content::StorageWriteRequest(r)) = m.1.content {
                        assert_eq!(1, m.1.command_id);
                        assert_eq!(r.file.data, data[index..index+chunk.file_byte_count]);
                        index += chunk.file_byte_count;
                    } else {
                        panic!("wrong type of protobuf message");
                    }
                },
                Err(e) => {
                    panic!("error {:?}", e);
                }
            };
        }
    }

    #[test]
    fn protobuf_codec_read_request_test() {
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        let path = "/ext/apps/GPIO/ublox.fap";
        let mut read_chunks = p.create_read_request_packet(path).unwrap();
        
        let mut read_packet = Vec::new();
        read_chunks.iter_mut().for_each(|x| read_packet.append(&mut *x));
        
        match ProtobufCodec::parse_response(&read_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::StorageReadRequest(r)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                    assert_eq!(path, r.path);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }

    #[test]
    pub fn protobuf_codec_stat_request_test() {
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        let path = "/ext/apps/GPIO/ublox.fap";
        let mut stat_chunks = p.create_stat_request_packet(path).unwrap();

        let mut stat_packet = Vec::new();
        stat_chunks.iter_mut().for_each(|x| stat_packet.append(&mut *x));
        
        match ProtobufCodec::parse_response(&stat_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::StorageStatRequest(r)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                    assert_eq!(path, r.path);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }

    #[test]
    pub fn protobuf_codec_delete_request_test() {
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        let path = "/ext/apps/GPIO/ublox.fap";
        let mut delete_chunks = p.create_delete_request_packet(path, true).unwrap();

        let mut delete_packet = Vec::new();
        delete_chunks.iter_mut().for_each(|x| delete_packet.append(&mut *x));
        
        match ProtobufCodec::parse_response(&delete_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::StorageDeleteRequest(r)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                    assert_eq!(path, r.path);
                    assert_eq!(true, r.recursive);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }

    #[test]
    pub fn protobuf_codec_alert_request_test() {
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        let alert_packet = p.create_alert_request_packet().unwrap();
        
        match ProtobufCodec::parse_response(&alert_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::SystemPlayAudiovisualAlertRequest(_)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                } else {
                    panic!("unexpected type decoded!");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }


    #[test]
    pub fn protobuf_codec_set_datetime_request_test() {
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        let datetime = chrono::DateTime::parse_from_rfc2822("Mon, 29 Jan 2024 10:39:45 -0700").unwrap();
        let datetime_packet = p.create_set_datetime_request_packet(datetime).unwrap();

        match ProtobufCodec::parse_response(&datetime_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::SystemSetDatetimeRequest(r)) = m.1.content {
                    assert_eq!(r.datetime.hour, datetime.hour());
                    assert_eq!(r.datetime.minute, datetime.minute());
                    assert_eq!(r.datetime.second, datetime.second());
                    assert_eq!(r.datetime.day, datetime.day());
                    assert_eq!(r.datetime.month, datetime.month());
                    assert_eq!(r.datetime.year, datetime.year() as u32);
                    assert_eq!(r.datetime.weekday, datetime.weekday().number_from_monday());
                    assert_eq!(1, m.1.command_id);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }

    #[test]
    pub fn protobuf_codec_get_datetime_request_test() {
        let mut p = ProtobufCodec::new();
        p.inc_command_id();
        let datetime_packet = p.create_get_datetime_request_packet().unwrap();

        match ProtobufCodec::parse_response(&datetime_packet) {
            Ok(m) => {
                if let Some(flipper_pb::flipper::main::Content::SystemGetDatetimeRequest(_)) = m.1.content {
                    assert_eq!(1, m.1.command_id);
                } else {
                    panic!("wrong type of protobuf message");
                }
            },
            Err(e) => {
                panic!("error {:?}", e);
            }
        };
    }
    
    #[test]
    fn bad_data_test() {
        // force the whole thing to u8
        // the original data is from a StorageListRequest of "/ext/apps/NFC/"
        //let dat = [18u8, 58, 16, 10, 14, 47, 101, 120, 116, 47, 97, 112, 112, 115, 47, 78, 70, 67, 47];
        let dat = [18u8, 0, 16, 10, 14, 47, 101, 120, 116, 47, 97, 112, 112, 115, 47, 78, 70, 67, 47];

        match ProtobufCodec::parse_response(&dat) {
            Ok(_) => {
                panic!("parse of bad data succeeded!");
            },
            Err(_) => {
                // nothing to do here
            }
        };
    }

}
