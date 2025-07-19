use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(&["proto/agent_message.proto"], &["proto/"])?;
    Ok(())
}