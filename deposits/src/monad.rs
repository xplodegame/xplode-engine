use alloy_primitives::{Address, U256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_signer::{LocalWallet, Signer};
use alloy_transport_http::Http;
use std::str::FromStr;
use std::sync::Arc;

pub async fn transfer_funds(
    from_private_key: &str,
    to_address: &str,
    amount_in_eth: f64,
    rpc_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Convert ETH to Wei
    let amount_in_wei = U256::from((amount_in_eth * 1e18) as u64);

    // Parse the recipient address
    let recipient = Address::from_str(to_address)?;

    // Set up the wallet using the private key
    let wallet = LocalWallet::from_str(from_private_key)?;
    let from_address = wallet.address();

    // Create a provider
    let transport = Http::new(rpc_url)?;
    let provider = ProviderBuilder::new().with_transport(transport).build()?;
    let provider = Arc::new(provider);

    // Get the current nonce for the sender
    let nonce = provider.get_transaction_count(from_address, None).await?;

    // Get current gas price
    let gas_price = provider.get_gas_price().await?;

    // Create the transaction
    let tx = alloy_network::eip1559::Transaction {
        to: Some(recipient),
        value: amount_in_wei,
        gas_limit: U256::from(21000), // Standard gas limit for ETH transfers
        max_fee_per_gas: gas_price,
        max_priority_fee_per_gas: gas_price,
        nonce,
        input: vec![],
        ..Default::default()
    };

    // Sign the transaction
    let signature = wallet.sign_transaction(&tx).await?;
    let signed_tx = tx.into_signed(signature);

    // Send the transaction
    let tx_hash = provider.send_raw_transaction(signed_tx.rlp()).await?;

    Ok(format!("Transaction sent: {}", tx_hash))
}
