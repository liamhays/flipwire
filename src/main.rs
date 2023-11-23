// TODO: move away from protoc-bin-vendored

// These are declared globally so that everything can use them,
// especially `flipper_pb`.
mod flipper_pb;
mod flipper_ble;
mod protobuf_codec;

use std::path::PathBuf;
use std::process;
use std::env;

// We have to use tokio, even though it's big, because bluez-async relies on it.
use tokio;
use clap::{Parser, Subcommand};

extern crate pretty_env_logger;
#[macro_use] extern crate log;

// other potential operations: set datetime, play AV alert, get screen frame, 
#[derive(Subcommand, Debug)]
enum Commands {
    /// Upload a local file to the Flipper
    Upload {
        /// Local file to upload
        file: PathBuf,
        /// Full Flipper path including filename to upload to
        dest: String,
    },
    /// Download a file from the Flipper
    Download {
        /// Flipper file to download
        file: String,
        /// Destination path on computer including filename
        dest: PathBuf,
    },
    /// Upload a file to the Flipper and attempt to launch it as an app
    Ul {
        /// Local file to upload
        file: PathBuf,
        /// Full Flipper path including filename to upload to
        dest: String
    },
    /// Launch an app on the Flipper
    Launch {
        /// A full path ("/ext/apps/...") or the name of a built-in
        /// app (i.e., "NFC")
        app: String,
    },
    
    /// Get a file listing of a Flipper directory
    Ls {
        #[arg(default_value="/ext")]
        path: String,
    },

    /// Play the Flipper's buzzing and flashing alert
    Alert {

    },

    /// Set the Flipper's time and date to the computer's current time
    /// and date
    Synctime {

    },
    
}

#[derive(Parser, Debug)]
//#[command(author, version, about, long_about = None)] // read from Cargo.toml
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Unique Flipper name, like "Uwu2" for "Flipper Uwu2" (required!)
    #[arg(short)]
    flipper_name: String,

    /// Disconnect from Flipper after all operations finish
    #[arg(short)]
    disconnect: bool,
}


// Most of the work (including printing things like status and
// progress bars) is done by flipper_ble.
#[tokio::main]
async fn main() {
    // pls don't judge
    // info log level is useful and I use it for most of the status messages
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }

    pretty_env_logger::init();
    debug!("start frl");

    let cli = Cli::parse();
    
    // All commands need a connected Flipper, so we start with that.
    let mut flipper =
        match flipper_ble::FlipperBle::connect_paired_device(&cli.flipper_name).await {
            Ok(d) => d,
            Err(e) => {
                error!("error finding Flipper {}: {}", cli.flipper_name, e);
                
                // process::exit() returns ! so it's compatible here
                process::exit(1)
            },
        };

    match &cli.command {
        Commands::Ls { path } => {
            match flipper.list(path).await {
                Ok(()) => {

                },
                Err(e) => {
                    error!("failed to list path: {}", e);
                }
            };
        },
        
        Commands::Launch { app } => {
            match flipper.launch(app).await {
                Ok(()) => {
                    info!("launched app successfully");
                },
                Err(e) => {
                    error!("failed to launch app {:?}: {}", app, e);
                }
            };
        },
        
        Commands::Download { file, dest } => {
            match flipper.download_file(file, dest).await {
                Ok(()) => {
                    info!("downloaded file successfully");
                },
                Err(e) => {
                    error!("failed to download file {:?}: {}", file, e);
                }
            };
        },
        
        Commands::Upload { file, dest } => {
            match flipper.upload_file(file, dest).await {
                Ok(()) => {
                    info!("sent file successfully");
                },
                Err(e) => {
                    error!("failed to send file: {}", e);
                }
            };
        },
        Commands::Ul { file, dest } => {
            // TODO: notification stream or something to get status
            // updates as each chunk is sent
            match flipper.upload_file(file, dest).await {
                Ok(()) => {
                    info!("sent file successfully");
                },
                Err(e) => {
                    error!("failed to send file: {}", e);
                }
            };

            match flipper.launch(dest).await {
                Ok(()) => {
                    info!("file started!");
                },
                Err(e) => {
                    error!("failed to launch app: {}", e);
                }
            };
        },
        Commands::Alert {} => {
            match flipper.alert().await {
                Ok(()) => {
                    info!("alert sent!");
                },
                Err(e) => {
                    error!("failed to send alert: {}", e);
                },
            };
        },
        Commands::Synctime {} => {
            match flipper.sync_datetime().await {
                Ok(()) => {
                    info!("Flipper date and time set!");
                },
                Err(e) => {
                    error!("failed to set Flipper date and time: {}", e);
                },
            };
        },
    }
    
    // disconnect if specified
    if cli.disconnect {
        debug!("disconnecting");
        match flipper.disconnect().await {
            Ok(()) => {},
            Err(e) => {
                error!("failed to disconnect from Flipper: {}", e);
            }
        }
    }
}

