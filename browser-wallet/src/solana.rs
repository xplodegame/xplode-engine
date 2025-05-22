use std::env;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_instruction,
    transaction::Transaction,
};

pub async fn withdraw_funds_to_user(
    recipient_address: Pubkey,
    amount_in_lamports: u64,
) -> anyhow::Result<String> {
    // Configure the client to use the Solana devnet or mainnet
    let rpc_url = env::var("SOLANA_RPC_URL").unwrap(); // or "https://api.devnet.solana.com" for devnet
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    let working_dir = env::current_dir().unwrap();
    let keypair_path = working_dir.join("treasury-keypair.json");
    // println!("Keypair path: {}", keypair_path.display());
    // // Load your keypair from file
    let sender_keypair = read_keypair_file(keypair_path).expect("Failed to load keypair");

    // Get the recent blockhash
    let recent_blockhash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    // Create a transfer instruction
    let instruction = system_instruction::transfer(
        &sender_keypair.pubkey(),
        &recipient_address,
        amount_in_lamports,
    );

    // Create a transaction
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&sender_keypair.pubkey()),
        &[&sender_keypair],
        recent_blockhash,
    );

    // // Send and confirm transaction
    // match client.send_and_confirm_transaction(&transaction) {
    //     Ok(signature) => Ok(signature.to_string()),
    //     Err(err) => Err(anyhow::anyhow!("Transaction failed: {}", err)),
    // }

    let tx_hash = client.send_and_confirm_transaction(&transaction).await?;
    Ok(tx_hash.to_string())
}
