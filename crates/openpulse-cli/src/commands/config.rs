/// Handler for `openpulse config init`.
pub fn run_init() -> anyhow::Result<()> {
    print!("{}", openpulse_config::init_template());
    Ok(())
}
