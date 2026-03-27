use ib_data_collector::database::{ParquetDatabase, DatabaseRepository};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = PathBuf::from("/Users/mohamedali/trading_project/ib-data-collector/ib_data");
    let db = ParquetDatabase::open(&data_dir)?;

    println!("=== Checking VWAV data availability ===\n");

    // Check all symbols
    let all_symbols = db.get_symbols()?;
    println!("Total symbols in database: {}", all_symbols.len());
    println!("VWAV in symbols list: {}", all_symbols.contains(&"VWAV".to_string()));

    // Check equity data
    let has_equity = db.has_equity_data("VWAV")?;
    println!("\nVWAV has equity data: {}", has_equity);

    if has_equity {
        let equity_range = db.equity_date_range("VWAV")?;
        println!("Equity date range: {:?}", equity_range);

        let bars = db.read_equity_bars("VWAV")?;
        println!("Number of equity bars: {}", bars.len());
        if !bars.is_empty() {
            println!("First bar: {:?}", bars.first());
            println!("Last bar: {:?}", bars.last());
        }
    }

    // Check options data
    let option_range = db.symbol_date_range("VWAV")?;
    println!("\nOptions date range: {:?}", option_range);

    let contracts = db.get_symbol_contracts("VWAV")?;
    println!("Number of option contracts: {}", contracts.len());

    if !contracts.is_empty() {
        println!("\nSample contracts:");
        for contract in contracts.iter().take(5) {
            println!("  - {:?} {} {:?} {:?}",
                contract.contract.expiration,
                contract.contract.strike,
                contract.contract.option_type,
                contract.contract.bar_type
            );
        }
    }

    Ok(())
}
