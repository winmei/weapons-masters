fn main() {
    let proto_file = "../../../proto/game_messages.proto";
    println!("cargo:rerun-if-changed={}", proto_file);
    
    prost_build::compile_protos(&[proto_file], &["../../../proto"])
        .unwrap_or_else(|e| panic!("Failed to compile Protobuf files: {}", e));
}
