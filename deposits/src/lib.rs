use redis::Client;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::AccountMeta, pubkey::Pubkey,
    signature::Keypair, signer::Signer, system_instruction, system_program,
    transaction::Transaction,
};
use std::{path::Path, str::FromStr, sync::Arc};

async fn handle_deposit(
    connection: Arc<RpcClient>,
    treasury: Arc<Keypair>,
    program_id: Pubkey,
    redis: Arc<Client>,
    deposit_address: Pubkey,
    amount: u64,
) -> anyhow::Result<()> {
    let mut conn = redis.get_connection()?;
    let user_id: String = redis::cmd("HGET")
        .arg("deposit_addresses")
        .arg(deposit_address.to_string())
        .query(&mut conn)?;

    let user_pubkey = Pubkey::from_str(&user_id)?;

    let instruction = anchor_lang::solana_program::instruction::Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(deposit_address, false), // PDA is not a signer
            AccountMeta::new(user_pubkey, false),
            AccountMeta::new(treasury.pubkey(), true), // Treasury is signer
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: {
            let mut data = vec![91, 60, 51, 162, 44, 140, 96, 24];
            data.extend_from_slice(&amount.to_le_bytes());
            data
        },
    };

    let recent_blockhash = connection.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&treasury.pubkey()),
        &[treasury.as_ref()], // Only treasury signs
        recent_blockhash,
    );

    let signature = connection.send_and_confirm_transaction(&transaction)?;

    println!("Confirmation sent: {:?}", signature);
    Ok(())
}
#[derive(Clone)]
pub struct DepositService {
    redis: Arc<Client>,
    connection: Arc<RpcClient>,
    treasury: Arc<Keypair>,
    program_id: Pubkey,
}

impl DepositService {
    pub fn new<P: AsRef<Path>>(treasury_keypair_path: P, program_id: Pubkey) -> Self {
        let connection = RpcClient::new_with_commitment(
            std::env::var("SOLANA_RPC_URL").unwrap(),
            CommitmentConfig::confirmed(),
        );

        let treasury_data = std::fs::read_to_string(treasury_keypair_path).unwrap();
        let treasury_bytes: Vec<u8> = serde_json::from_str(&treasury_data).unwrap();
        let treasury = Keypair::from_bytes(&treasury_bytes).unwrap();

        Self {
            redis: Arc::new(Client::open(std::env::var("REDIS_URL").unwrap()).unwrap()),
            connection: Arc::new(connection),
            treasury: Arc::new(treasury),
            //program_id: Pubkey::from_str("FFT8CyM7DnNoWG2AukQqCEyNtZRLJvxN9WK6S7mC5kLP").unwrap(),
            program_id,
        }
    }

    pub fn generate_deposit_address(&self) -> anyhow::Result<Pubkey> {
        let new_keypair = Keypair::new();
        let user_pubkey = new_keypair.pubkey();
        let (pda, _) =
            Pubkey::find_program_address(&[b"deposit", user_pubkey.as_ref()], &self.program_id);

        let mut conn = self.redis.get_connection()?;
        redis::cmd("HSET")
            .arg("deposit_addresses")
            .arg(pda.to_string())
            .arg(user_pubkey.to_string())
            .exec(&mut conn)?;
        Ok(pda)
    }

    pub async fn check_deposits(&self, pubkeys: Vec<Pubkey>) -> anyhow::Result<()> {
        if let Ok(accounts) = self.connection.get_multiple_accounts(&pubkeys) {
            for (i, account) in accounts.iter().enumerate() {
                // check if account lamport is > 0, initiate fund transfer to the treasury
                if let Some(account) = account {
                    if account.lamports > 0 {
                        // handle deposit
                        println!("Account: {:?}", account);
                        let conn = self.connection.clone();
                        let treasury = self.treasury.clone();
                        let redis = self.redis.clone();
                        let program_id = self.program_id.clone();
                        let pubkey = pubkeys[i].clone();
                        let amount = account.lamports.clone();
                        tokio::spawn(async move {
                            if let Err(err) =
                                handle_deposit(conn, treasury, program_id, redis, pubkey, amount)
                                    .await
                            {
                                eprintln!("Error: {:?}", err);
                            }
                        });
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn withdraw_to_user_from_treasury(
        &self,
        withdrawal_address: String,
        amount: u64,
    ) -> anyhow::Result<String> {
        let to_pubkey = Pubkey::from_str(&withdrawal_address)?;

        let treasury_pubkey = self.treasury.pubkey();
        let treasury_keypair = self.treasury.clone();
        let rpc_client = self.connection.clone();

        let signature = tokio::task::spawn_blocking(move || {
            let instruction = system_instruction::transfer(&treasury_pubkey, &to_pubkey, amount);
            let recent_blockhash = rpc_client.get_latest_blockhash()?; // Blocking
            let transaction = Transaction::new_signed_with_payer(
                &[instruction],
                Some(&treasury_pubkey),
                &[treasury_keypair.as_ref()],
                recent_blockhash,
            );

            let signature = rpc_client.send_and_confirm_transaction(&transaction)?; // Blocking
            Ok::<_, anyhow::Error>(signature.to_string())
        })
        .await??;

        println!("Signature: {:?}", signature);
        Ok(signature)
    }
}

// // pub async fn read_account_updates(&self, account_pubkey: Pubkey) -> anyhow::Result<()> {
// //     let url = "wss://api.devnet.solana.com/";

// //     let connection = self.connection.clone();
// //     let treasury = self.treasury.clone();
// //     let program_id = self.program_id;
// //     let redis = self.redis.clone();

// //     // let ws_url = std::env::var("SOLANA_WS_URL").unwrap_or_else(|_| {
// //     //     std::env::var("SOLANA_RPC_URL")
// //     //         .unwrap()
// //     //         .replace("http", "ws")
// //     // });

// //     tokio::spawn(async move {
// //         loop {
// //             println!("Reconnecting ...");
// //             let (subscription, mut account_subscription_receiver) =
// //                 PubsubClient::account_subscribe(
// //                     url,
// //                     &account_pubkey,
// //                     Some(RpcAccountInfoConfig {
// //                         encoding: None,
// //                         data_slice: None,
// //                         commitment: Some(CommitmentConfig::confirmed()),
// //                         min_context_slot: None,
// //                     }),
// //                 )
// //                 .unwrap();
// //             let _sub = subscription;
// //             loop {
// //                 match account_subscription_receiver.recv() {
// //                     Ok(response) => {
// //                         // println!("account subscription response: {:?}", response);
// //                         if response.value.lamports > 0 {
// //                             if let Err(e) = handle_deposit(
// //                                 &connection,
// //                                 &treasury,
// //                                 program_id,
// //                                 &redis,
// //                                 account_pubkey,
// //                                 response.value.lamports,
// //                             )
// //                             .await
// //                             {
// //                                 eprintln!("Error handling deposit: {}", e);
// //                             }
// //                         }
// //                     }
// //                     Err(e) => {
// //                         println!("account subscription error: {:?}", e);
// //                         break;
// //                     }
// //                 }
// //             }
// //         }
// //     });

// //     Ok(())
// // }

// pub async fn read_account_updates(&self, account_pubkey: Pubkey) -> anyhow::Result<()> {
//     let url = "wss://api.devnet.solana.com/";
//     loop {
//         let (subscription, account_subscription_receiver) = PubsubClient::account_subscribe(
//             url,
//             &account_pubkey,
//             Some(RpcAccountInfoConfig {
//                 encoding: None,
//                 data_slice: None,
//                 commitment: Some(CommitmentConfig::confirmed()),
//                 min_context_slot: None,
//             }),
//         )?;
//         let _sub = subscription;

//         while let Ok(response) = account_subscription_receiver.recv() {
//             // Use async recv
//             if response.value.lamports > 0 {
//                 handle_deposit(
//                     &self.connection,
//                     &self.treasury,
//                     self.program_id,
//                     &self.redis,
//                     account_pubkey,
//                     response.value.lamports,
//                 )
//                 .await?;
//             }
//         }
//         // Reconnect if the WebSocket drops
//         tokio::time::sleep(Duration::from_secs(5)).await;
//     }
// }
