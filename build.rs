//use prost_build;
    
fn main() {
    // Use this in build.rs
    protobuf_codegen::Codegen::new()
    // Use `protoc` parser, optional.
        .protoc()
    // Use `protoc-bin-vendored` bundled protoc command, optional.
        .protoc_path(&protoc_bin_vendored::protoc_bin_path().unwrap())
    // All inputs and imports from the inputs must reside in `includes` directories.
        .includes(&["flipperzero-protobuf"])
    // Inputs must reside in some of include paths.
        .input("flipperzero-protobuf/application.proto")
        .input("flipperzero-protobuf/desktop.proto")
        .input("flipperzero-protobuf/flipper.proto")
        .input("flipperzero-protobuf/gpio.proto")
        .input("flipperzero-protobuf/gui.proto")
        .input("flipperzero-protobuf/property.proto")
        .input("flipperzero-protobuf/storage.proto")
        .input("flipperzero-protobuf/system.proto")

    // Specify output directory relative to Cargo output directory.
        .cargo_out_dir("protos")
        .run_from_script();
}
