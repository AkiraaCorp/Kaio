use starknet::core::types::{BlockId, EventFilter, Felt};
use starknet::core::utils::get_selector_from_name;
use starknet::providers::{jsonrpc::HttpTransport, JsonRpcClient, Provider};
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use tokio;
use tokio::time::{sleep, Duration};
use num_bigint::BigUint;
use dotenv::dotenv;
use std::env;
use url::Url;
use sqlx::types::BigDecimal;
use sqlx::postgres::PgRow;
use sqlx::Row;
use num_traits::cast::ToPrimitive;
use std::str::FromStr;
use env_logger::Env;
use log::{info, error };

#[derive(Debug)]
struct UserBet {
    bet: bool,
    amount: BigUint,
    has_claimed: bool,
    claimable_amount: BigUint,
    user_odds: Odds,
    user_address: String,
}

#[derive(Debug)]
struct Odds {
    no_probability: u64,
    yes_probability: u64,
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    println!(
        "                                                             
                     //   / /                                 
                    //__ / /      ___       ( )      ___      
                   //__  /      //   ) )   / /     //   ) )   
                  //   \\ \\     //   / /   / /     //   / /    
                 //     \\ \\   ((___( (   / /     ((___/ /     
        " 
    );
    
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let endpoint = env::var("RPC_ENDPOINT").expect("RPC_ENDPOINT must be set");

    let rpc_url = Url::parse(&endpoint)
        .expect("Invalid RPC URL"); 
    let transport = HttpTransport::new(rpc_url);
    let provider = JsonRpcClient::new(transport);

    let pool = setup_database().await;

    let contract_addresses: Vec<Felt> = sqlx::query("SELECT address FROM events WHERE is_active = true")
    .map(|row: PgRow| {
        let address: String = row.get("address");
        Felt::from_hex(&address).expect("Invalid Felt")
    })
    .fetch_all(&pool)
    .await
    .expect("Failed to fetch contract addresses");

    let fetched_addresses = contract_addresses.iter().map(|x| x.clone()).collect::<Vec<Felt>>();
    println!("Fetched addresses: {:?}", fetched_addresses);

    loop {
        process_new_events(&provider, &fetched_addresses, &pool).await;
        sleep(Duration::from_secs(10)).await;
    }
}

async fn setup_database() -> Pool<Postgres> {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool");

  
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS block_state (
            id INTEGER PRIMARY KEY,
            last_processed_block BIGINT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create block_state table");

    sqlx::query(
        "INSERT INTO block_state (id, last_processed_block)
         VALUES (1, 0)
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("Failed to initialize block_state");

    pool
}

async fn get_last_processed_block(pool: &Pool<Postgres>) -> u64 {
    let row: (i64,) = sqlx::query_as("SELECT last_processed_block FROM block_state WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("Failed to fetch last_processed_block");

    row.0 as u64
}

async fn update_last_processed_block(pool: &Pool<Postgres>, block_number: u64) {
    sqlx::query("UPDATE block_state SET last_processed_block = $1 WHERE id = 1")
        .bind(block_number as i64)
        .execute(pool)
        .await
        .expect("Failed to update last_processed_block");
}

async fn process_new_events(
    provider: &JsonRpcClient<HttpTransport>,
    contract_addresses: &[Felt],
    pool: &Pool<Postgres>,
) {
    let last_processed_block = get_last_processed_block(pool).await;
    let latest_block = provider
        .block_number()
        .await
        .expect("Failed to get latest block number");

    //le last block mettre direct en dur dans la DB pour le premier pour ne pas avoir a tout sync from 0
    info!("Last processed block: {}", last_processed_block);
    info!("Latest block: {}", latest_block);

    if latest_block > last_processed_block {
        info!(
            "🔀 Processing blocks from {} to {}",
            last_processed_block + 1,
            latest_block
        );
        for block_number in (last_processed_block + 1)..=latest_block {
            for contract_address in contract_addresses {
                process_block(provider, block_number, *contract_address, pool).await;
            }
        }
        update_last_processed_block(pool, latest_block).await;
    } else {
        info!(" 📡 No new blocks to process.");
    }
}

async fn process_block(
    provider: &JsonRpcClient<HttpTransport>,
    block_number: u64,
    contract_address: Felt,
    pool: &Pool<Postgres>,
) {
    info!(
        "Fetching events for block {} on contract {}",
        block_number, contract_address
    );

    let filter = EventFilter {
        from_block: Some(BlockId::Number(block_number)),
        to_block: Some(BlockId::Number(block_number)),
        address: Some(contract_address),
        keys: Some(vec![vec![bet_placed_event_key()]]),
    };

    let chunk_size = 100;
    let events_page = match provider.get_events(filter, None, chunk_size).await {
        Ok(page) => page,
        Err(err) => {
            error!("Error fetching events: {}", err);
            return;
        }
    };

    info!("Number of events fetched: {}", events_page.events.len());

    if events_page.events.is_empty() {
        info!(
            "No events found for block {} on contract {}",
            block_number, contract_address
        );
    }

    for event in events_page.events {
    
        if let Some(decoded_event) = parse_bet_placed_event(&event.data) {
            info!("✨ New BetPlace event: {:?}", decoded_event);
            store_event(
                pool,
                &decoded_event,
                block_number,
                &event.transaction_hash.to_fixed_hex_string(),
                &event.from_address.to_fixed_hex_string(),
            )
            .await;
        } else {
            info!("❌Failed to parse BetPlace event");
        }
    }
}

fn field_element_to_u64(fe: Felt) -> u64 {
    fe.to_biguint().to_u64().unwrap()
}


fn parse_bet_placed_event(data: &[Felt]) -> Option<UserBet> {
    info!("Event data length: {}", data.len());

    if data.len() >= 6 {
        let bet = data[0];
        let amount_felt = data[1];
        let has_claimed = data[3];
        let claimable_amount_felt = data[4];
        let no_probability = data[6];
        let yes_probability = data[8];
        let user_address = data[10].to_fixed_hex_string();

        let bet_bool = field_element_to_bool(bet);
        let amount = amount_felt.to_biguint();
        let has_claimed_bool = field_element_to_bool(has_claimed);
        let claimable_amount = claimable_amount_felt.to_biguint();

        let no_probability_value = field_element_to_u64(no_probability);
        let yes_probability_value = field_element_to_u64(yes_probability);

        let user_bet = UserBet {
            bet: bet_bool,
            amount,
            has_claimed: has_claimed_bool,
            claimable_amount,
            user_odds: Odds {
                no_probability: no_probability_value,
                yes_probability: yes_probability_value,
            },
            user_address,
        };

        Some(user_bet)
    } else {
        info!("Event data is too short.");
        None
    }
}


fn field_element_to_bool(fe: Felt) -> bool {
    fe != Felt::ZERO
}

fn bet_placed_event_key() -> Felt {
    get_selector_from_name("BetPlace").expect("Failed to compute event selector")
}

fn biguint_to_bigdecimal_scaled(value: &BigUint, scale: u32) -> BigDecimal {
    let value_str = value.to_str_radix(10);
    let big_decimal_value = BigDecimal::from_str(&value_str).unwrap();
    let scaling_factor = BigDecimal::from(10u64.pow(scale));
    big_decimal_value / scaling_factor
}

async fn store_event(
    pool: &Pool<Postgres>,
    event: &UserBet,
    block_number: u64,
    transaction_hash: &str,
    from_address: &str,
) {
    let bet:i32 = event.bet.into();
    let is_claimable = false;
    sqlx::query(
        "INSERT INTO bets (
            bet,
            amount,
            is_claimable,
            has_claimed,
            claimable_amount,
            no_probability,
            yes_probability,
            user_address,
            block_number,
            transaction_hash,
            \"event_address\"
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (block_number, transaction_hash) DO NOTHING",
    )
    .bind(bet)
    .bind(biguint_to_bigdecimal_scaled(&event.amount, 18)) 
    .bind(is_claimable)    
    .bind(event.has_claimed)
    .bind(biguint_to_bigdecimal_scaled(&event.claimable_amount, 18)) 
    .bind(event.user_odds.no_probability as i64)
    .bind(event.user_odds.yes_probability as i64)
    .bind(&event.user_address)
    .bind(block_number as i64)
    .bind(transaction_hash)
    .bind(from_address)
    .execute(pool)
    .await
    .expect("Failed to store event in database");
}