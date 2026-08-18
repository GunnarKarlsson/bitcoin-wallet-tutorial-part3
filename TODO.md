Alternative: Update code to work with your current (descriptor) walletIf you prefer not to create a legacy wallet (avoids deprecation warning), change your Rust code to use import_multi with a descriptor. This is the modern way.Add/update these:rust

use bitcoincore_rpc::json::{ImportMultiRequest, ImportMultiOptions, ImportMultiRescanSince};

// Replace ensure_address_imported with this
fn ensure_descriptor_imported(client: &Client, privkey_wif: &str) -> Result<()> {
    let descriptor = format!("combo({})", privkey_wif);  // combo watches P2PKH + SegWit variants

    let request = ImportMultiRequest {
        desc: descriptor,
        timestamp: ImportMultiRescanSince::Now,
        watchonly: Some(true),
        label: Some("rust-wallet".to_string()),
        ..Default::default()
    };

    let options = ImportMultiOptions {
        rescan: Some(false),
    };

    let results = client.import_multi(&[request], Some(options))?;

    for res in results {
        if let Some(err) = res.error {
            let msg = err.message.to_lowercase();
            if !msg.contains("already") && !msg.contains("exists") {
                return Err(anyhow::anyhow!("import_multi error: {:?}", err));
            }
        }
    }

    println!("Imported descriptor for watch-only (combo).");
    Ok(())
}

// Update get_balance to take WIF and call the new function
fn get_balance(client: &Client, address: &str, private_key_wif: &str) -> Result<f64> {
    ensure_descriptor_imported(client, private_key_wif)?;

    let addr: BtcAddress = address
        .parse::<Address<NetworkUnchecked>>()
        .context("Invalid address")?
        .assume_checked();

    let utxos = client.list_unspent(
        Some(0),
        None,
        Some(&[&addr]),
        Some(true),
        None,
    )?;

    let total: f64 = utxos.iter().map(|utxo| utxo.amount.to_btc()).sum();
    Ok(total)
}

In main's Balance branch:rust

let balance = get_balance(&client, &wallet.address, &wallet.private_key)?;

