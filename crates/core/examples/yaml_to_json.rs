// CDK公式bootstrapテンプレートをJSON化する管理用ユーティリティ。
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("YAMLファイルを指定してください")?;
    let value: serde_json::Value = serde_yaml_ng::from_reader(std::fs::File::open(path)?)?;
    serde_json::to_writer_pretty(std::io::stdout(), &value)?;
    Ok(())
}
