use anyhow::{Context, Result};
use bitcoin::address::NetworkUnchecked;
use bitcoin::{Address, Network, PrivateKey, PublicKey};
use bitcoincore_rpc::bitcoin::Address as BtcAddress;
use bitcoincore_rpc::json::{ImportDescriptors, ImportMultiResult, Timestamp};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use secp256k1::{Secp256k1, rand::rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
struct Wallet {
    private_key: String, // WIF format
    address: String,
}

fn wallet_path() -> Result<PathBuf, std::io::Error> {
    std::env::current_dir().map(|mut path| {
        path.push("wallet.json");
        path
    })
}

fn save_wallet(wallet: &Wallet) -> Result<()> {
    let path = wallet_path()?;
    let json = serde_json::to_string_pretty(wallet)?;
    fs::write(&path, json)?;
    println!("Wallet saved to: {}", path.display());
    Ok(())
}

fn load_wallet() -> Result<Wallet> {
    let path = wallet_path()?;
    let json = fs::read_to_string(&path).context("No wallet found. Run 'new' to create one.")?;
    let wallet: Wallet = serde_json::from_str(&json)?;
    Ok(wallet)
}

fn generate_new_wallet() -> Result<Wallet> {
    let secp = Secp256k1::new();
    let mut rng = OsRng;
    let (secret_key, public_key) = secp.generate_keypair(&mut rng);

    let privkey = PrivateKey::new(secret_key, Network::Testnet);
    let address = Address::p2pkh(PublicKey::new(public_key), Network::Testnet);

    let wallet = Wallet {
        private_key: privkey.to_wif(),
        address: address.to_string(),
    };

    println!("New wallet created!");
    println!("Address (send testnet BTC here): {}", wallet.address);
    println!("Private Key (WIF) — KEEP SECRET!: {}", wallet.private_key);

    save_wallet(&wallet)?;
    Ok(wallet)
}

fn get_rpc_client() -> Result<Client> {
    let rpc_url = "http://127.0.0.1:18332"; // your local testnet node
    let auth = Auth::UserPass("admin".to_string(), "abc123".to_string());
    Client::new(rpc_url, auth).context("Failed to connect to Bitcoin Core RPC")
}

fn ensure_descriptor_imported(client: &Client, address: &str) -> Result<()> {
    // Use addr() descriptor — pure watch-only, no private key involved

    let plain_desc = format!("addr({})", address);

    // Step 1: Get checksummed descriptor via getdescriptorinfo
    let info = client.get_descriptor_info(&plain_desc)
        .context("getdescriptorinfo failed (check if address matches network: testnet addresses start with m/n/tb1)")?;

    let checksummed_desc = info.descriptor; // This is "addr(... )#xxxxxxxx"

    let import_req = ImportDescriptors {
        descriptor: checksummed_desc, // ← matches your struct: descriptor (not desc)
        timestamp: Timestamp::Now,    // ← matches your struct: Timestamp::Now
        active: Some(false),          // don't make it the active descriptor for auto-generation
        range: None,                  // no range for combo()
        next_index: None,             // ← required by your struct; safe to set None for non-ranged
        internal: None,
        label: Some("rust-wallet".to_string()),
        // Add other fields from /* … */ if your struct requires them (e.g. internal: Some(false))
        // If the struct has more mandatory fields, fill them with defaults/None
        // Example if present: internal: Some(false),
    };

    // import_descriptors takes Vec<ImportDescriptors>
    let results: Vec<ImportMultiResult> = client.import_descriptors(import_req)?;

    for res in results {
        if let Some(err) = res.error {
            let msg = err.message.to_lowercase();
            if !msg.contains("already") && !msg.contains("exists") {
                return Err(anyhow::anyhow!("import_descriptors failed: {:?}", err));
            }
        } else if res.success {
            println!("Descriptor imported successfully (watch-only via combo).");
        }
    }

    Ok(())
}

fn get_balance(client: &Client, address: &str) -> Result<f64> {
    ensure_descriptor_imported(client, address)?;

    let addr: BtcAddress = address
        .parse::<Address<NetworkUnchecked>>()
        .context("Invalid address")?
        .assume_checked();
    let utxos = client.list_unspent(
        Some(0), // min confirmation
        None,    // max confirmations
        Some(&[&addr]),
        Some(true), // include_unsafe (for 0-conf)
        None,
    )?;

    let total: f64 = utxos.iter().map(|utxo| utxo.amount.to_btc()).sum();
    Ok(total)
}

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "btc-wallet", about = "Minimal Bitcoin testnet wallet in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new wallet
    New,
    /// Show current balance
    Balance,
    /// Show receive address
    Receive,
    Send {
        recipient: String,
        amount: f64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::New) => {
            generate_new_wallet()?;
        }
        Some(Commands::Balance) => {
            let wallet = load_wallet()?;
            let client = get_rpc_client()?;
            let balance = get_balance(&client, &wallet.address)?;
            println!("Balance: {:.8} tBTC", balance);
        }
        Some(Commands::Receive) | None => {
            let wallet = load_wallet()?;
            println!("Receive address: {}", wallet.address);
            println!("Tip: Paste this into a testnet faucet!");
        }
        Some(Commands::Send { recipient, amount }) => {
            let wallet = load_wallet()?;
            let client = get_rpc_client()?;
            let txid = send_tx(
                &client,
                &wallet.private_key,
                &wallet.address,
                &recipient,
                amount,
            )?;
            println!("Sent! TxID: {}", txid);
            println!("Check: https://mempool.space/testnet/tx/{}", txid);
        }
    }
    Ok(())
}

use bitcoin::{OutPoint, ScriptBuf};
use bitcoincore_rpc::bitcoin::Amount;
use itertools::Itertools;

#[derive(Clone, Debug)]
struct Utxo {
    outpoint: OutPoint,
    amount: Amount,
    script_pubkey: ScriptBuf,
}

fn get_utxos(client: &Client, address: &str) -> Result<Vec<Utxo>> {
    let addr: bitcoin::Address = address
        .parse::<Address<NetworkUnchecked>>()
        .context("Invalid address")?
        .assume_checked();
    let list = client.list_unspent(Some(0), None, Some(&[&addr]), Some(true), None)?;

    Ok(list
        .into_iter()
        .map(|u| Utxo {
            outpoint: OutPoint {
                txid: u.txid,
                vout: u.vout,
            },
            amount: u.amount,
            script_pubkey: u.script_pub_key.clone().into(),
        })
        .collect())
}

fn select_inputs(utxos: &[Utxo], target: Amount, fee_estimate: Amount) -> Result<Vec<Utxo>> {
    let mut selected = vec![];
    let mut total = Amount::ZERO;
    for utxo in utxos.iter().sorted_by_key(|u| u.amount) {
        // greedy smallest first
        selected.push(utxo.clone()); // Utxo is Clone
        total += utxo.amount;
        if total >= target + fee_estimate {
            break;
        }
    }
    if total < target + fee_estimate {
        anyhow::bail!("Insufficient funds");
    }
    Ok(selected)
}

use bitcoin::ecdsa::Signature as EcdsaSignature;
use bitcoin::{Psbt, Sequence, Transaction, TxIn, TxOut};

fn send_tx(
client: &Client,                    // Bitcoin Core RPC client for querying UTXOs and broadcasting
    privkey_wif: &str,                  // Wallet private key in Wallet Import Format (Base58Check string)
    address: &str,                      // Our own wallet address (used to find UTXOs and create change output)
    recipient: &str,                    // Destination address to send BTC to
    amount_btc: f64,                    // Amount to send, in BTC (floating point, will be converted to satoshis)
) -> Result<String> {                   // Returns Ok(txid hex string) or Err on failure
  // Parse the WIF private key string into a bitcoin::PrivateKey struct
    let pk = PrivateKey::from_wif(privkey_wif).context("Invalid WIF")?;
    // Extract the inner 32-byte secp256k1 SecretKey for actual signing
    let privkey = pk.inner;
    // Create a fresh secp256k1 context (required for key operations and signing)
    let secp = Secp256k1::new();

    // Parse recipient address string → bitcoin::Address (with unchecked network first, then assume testnet/mainnet)
    let recipient_addr: bitcoin::Address = recipient
        .parse::<Address<NetworkUnchecked>>()
        .context("Invalid recipient address")?
        .assume_checked();
    // Same for our own wallet address (used for change output and UTXO lookup)
    let my_addr: bitcoin::Address = address
        .parse::<Address<NetworkUnchecked>>()
        .context("Invalid address")?
        .assume_checked();
 // Convert user-provided BTC amount (float) to bitcoin::Amount (satoshis)
    let amount = Amount::from_btc(amount_btc)?;
    // Very rough static fee estimate in satoshis (~2 sat/vB × ~1000 vB virtual bytes)
    // In production, replace with client.estimate_smart_fee() RPC call
    let fee_sat = 2500; // rough estimate; improve later with estimatesmartfee

    // Fetch all spendable (confirmed) UTXOs belonging to our address via RPC
    let utxos = get_utxos(client, address)?;
    // Greedily select smallest UTXOs first until we have enough to cover send amount + fee
    let selected = select_inputs(&utxos, amount, Amount::from_sat(fee_sat))?;

    // Sum the value of all selected UTXOs (in satoshis)
    let total_in = selected.iter().map(|u| u.amount).sum::<Amount>();
    // Calculate change = total input value - send amount - fee
    let change = total_in - amount - Amount::from_sat(fee_sat);

    // Prepare vector of transaction outputs
    let mut outputs: Vec<TxOut> = vec![];

    // Recipient output: send the requested amount to the destination address
    // Recipient output
    outputs.push(TxOut {
        value: amount,
        script_pubkey: recipient_addr.script_pubkey(),  // Generates P2PKH / P2WPKH locking script depending on address type
    });

    // Change output (if > dust)
    if change >= Amount::from_sat(546) {
        outputs.push(TxOut {
            value: change,
            script_pubkey: my_addr.script_pubkey(),
        });
    }

    // Build unsigned tx
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: bitcoin::locktime::absolute::LockTime::ZERO,
        input: selected
            .iter()
            .map(|u| TxIn {
                previous_output: u.outpoint,
                script_sig: ScriptBuf::default(),
                sequence: Sequence::MAX,
                witness: bitcoin::witness::Witness::default(),
            })
            .collect(),
        output: outputs,
    };

    // Use PSBT to sign (P2PKH legacy: set witness_utxo for fee calc, then final_script_sig)
    let mut psbt = Psbt::from_unsigned_tx(tx)?;
    let pubkey = secp256k1::PublicKey::from_secret_key(&secp, &privkey);
    let bitcoin_pubkey = bitcoin::PublicKey::new(pubkey);

    // Simplified legacy P2PKH signing for tutorial purposes (single-sig, no SegWit).
    // In real code / multi-sig / mixed input types, use partial_sigs + a proper finalizer.
    // Here we manually build scriptSig and set final_script_sig directly — works fine for this example.
    // Assumes single-key wallet.
    for (i, utxo) in selected.iter().enumerate() {
        psbt.inputs[i].witness_utxo = Some(TxOut {
            // still useful for fee estimation tools
            value: utxo.amount,
            script_pubkey: utxo.script_pubkey.clone(),
        });

        let sighash_cache = bitcoin::sighash::SighashCache::new(&psbt.unsigned_tx);
        let sighash = sighash_cache.legacy_signature_hash(
            i,
            utxo.script_pubkey.as_script(),
            bitcoin::sighash::EcdsaSighashType::All.to_u32(),
        )?;

        let msg = secp256k1::Message::from(sighash);
        let sig = secp.sign_ecdsa(&msg, &privkey);

        // Wrap secp256k1 signature with SIGHASH_ALL using bitcoin's ECDSA helper type,
        // then serialize to a `SerializedSignature` which implements `AsRef<PushBytes>`.
        let sig_btc = EcdsaSignature::sighash_all(sig);

        let script_sig = bitcoin::script::Builder::new()
            .push_slice(sig_btc.serialize())
            .push_key(&bitcoin_pubkey)
            .into_script();

        psbt.inputs[i].final_script_sig = Some(script_sig);
    }

    // Extract works because we already finalized the inputs manually
    let signed_tx = psbt.extract_tx()?;

    match client.send_raw_transaction(&signed_tx) {
        Ok(txid) => {
            println!("Broadcasted transaction: {}", txid);
            Ok(txid.to_string())
        }
        Err(e) => {
            eprintln!("Broadcast failed: {}", e);
            Err(anyhow::anyhow!("Broadcast failed: {}", e))
        }
    }
}
