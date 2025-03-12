use alloy_network::TransactionBuilder;
use alloy_primitives::{Address, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::TransactionRequest;
use alloy_signer_local::PrivateKeySigner;
use std::{env, str::FromStr};

pub async fn transfer_funds(to_address: &str, amount_in_eth: f64) -> anyhow::Result<String> {
    let private_key = env::var("MONAD_ACCOUNT_PRIVATE_KEY").unwrap();
    let wallet = PrivateKeySigner::from_str(&private_key)?;
    let from_address = wallet.address();
    let rpc_url = env::var("MONAD_RPC_URL").unwrap();
    // Connect to an Ethereum node via RPC
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .on_http(rpc_url.parse().unwrap());

    // Define the recipient address
    let to_address = Address::from_str(to_address)?; // Replace with recipient address

    // Build a transaction to send 100 wei from Alice to Bob
    let tx = TransactionRequest::default()
        .with_from(from_address)
        .with_to(to_address)
        .with_value(U256::from((amount_in_eth * 10_u64.pow(18) as f64) as u64));

    // Send the transaction and listen for the transaction to be included.
    let tx_hash = provider.send_transaction(tx).await?.watch().await?;

    println!("Sent transaction: {tx_hash}");

    Ok(tx_hash.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_transfer_funds() -> anyhow::Result<()> {
        transfer_funds("0x0BF493537Fa5b08836d7AE8750CFEA682a0f190C", 0.01).await?;
        Ok(())
    }
}
