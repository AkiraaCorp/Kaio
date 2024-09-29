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
use num_traits::cast::ToPrimitive;
use std::str::FromStr;

#[derive(Debug)]
struct UserBet {
    bet: bool,
    amount: BigUint,
    has_claimed: bool,
    claimable_amount: BigUint,
    user_odds: Odds,
}

#[derive(Debug)]
struct Odds {
    no_probability: u64,
    yes_probability: u64,
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let rpc_url = Url::parse("https://starknet-sepolia.blastapi.io/05d8c1e9-70d6-41e4-a849-d2dff1e62b3b")
        .expect("Invalid RPC URL"); 
    let transport = HttpTransport::new(rpc_url);
    let provider = JsonRpcClient::new(transport);

    let contract_addresses = vec![
        Felt::from_hex("0x01244abdf52ee7eab1c40f34f25017efa4873d7c470da99d3799214b9754e454")
            .expect("Invalid contract address"),
        Felt::from_hex("0x03465a5b8edc64e400d1b32d7e684a9b4f9dbf99f9e643934e902371ab51b387")
            .expect("Invalid contract address"), // ici vecteur d'event, on add to nos contract a listen
    ];

    let pool = setup_database().await;

    loop {
        process_new_events(&provider, &contract_addresses, &pool).await;
        sleep(Duration::from_secs(30)).await;
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
        "CREATE TABLE IF NOT EXISTS bet_placed (
            id SERIAL PRIMARY KEY,
            bet BOOLEAN NOT NULL,
            amount NUMERIC(78, 18) NOT NULL,
            has_claimed BOOLEAN NOT NULL,
            claimable_amount NUMERIC(78, 18) NOT NULL,
            no_probability BIGINT NOT NULL,
            yes_probability BIGINT NOT NULL,
            block_number BIGINT NOT NULL,
            transaction_hash TEXT NOT NULL,
            from_address TEXT NOT NULL,
            UNIQUE (block_number, transaction_hash)
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create bet_placed table");

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_state (
            id INTEGER PRIMARY KEY,
            last_processed_block BIGINT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .expect("Failed to create app_state table");

    sqlx::query(
        "INSERT INTO app_state (id, last_processed_block)
         VALUES (1, 0)
         ON CONFLICT (id) DO NOTHING",
    )
    .execute(&pool)
    .await
    .expect("Failed to initialize app_state");

    pool
}

async fn get_last_processed_block(pool: &Pool<Postgres>) -> u64 {
    let row: (i64,) = sqlx::query_as("SELECT last_processed_block FROM app_state WHERE id = 1")
        .fetch_one(pool)
        .await
        .expect("Failed to fetch last_processed_block");

    row.0 as u64
}

async fn update_last_processed_block(pool: &Pool<Postgres>, block_number: u64) {
    sqlx::query("UPDATE app_state SET last_processed_block = $1 WHERE id = 1")
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
    println!("Last processed block: {}", last_processed_block);
    println!("Latest block: {}", latest_block);

    if latest_block > last_processed_block {
        println!(
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
        println!("No new blocks to process.");
    }
}

async fn process_block(
    provider: &JsonRpcClient<HttpTransport>,
    block_number: u64,
    contract_address: Felt,
    pool: &Pool<Postgres>,
) {
    println!(
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
            eprintln!("Error fetching events: {}", err);
            return;
        }
    };

    println!("Number of events fetched: {}", events_page.events.len());

    if events_page.events.is_empty() {
        println!(
            "No events found for block {} on contract {}",
            block_number, contract_address
        );
    }

    for event in events_page.events {
    
        if let Some(decoded_event) = parse_bet_placed_event(&event.data) {
            println!("✨ New BetPlace event: {:?}", decoded_event);
            store_event(
                pool,
                &decoded_event,
                block_number,
                &event.transaction_hash.to_fixed_hex_string(),
                &event.from_address.to_fixed_hex_string(),
            )
            .await;
        } else {
            println!("❌Failed to parse BetPlace event");
        }
    }
}

fn field_element_to_u64(fe: Felt) -> u64 {
    fe.to_biguint().to_u64().unwrap()
}


fn parse_bet_placed_event(data: &[Felt]) -> Option<UserBet> {
    println!("Event data length: {}", data.len());
    for (i, felt) in data.iter().enumerate() {
        println!("data[{}]: {}", i, felt);
    }

    if data.len() >= 6 {
        let bet = data[0];
        let amount_felt = data[1];
        let has_claimed = data[3];
        let claimable_amount_felt = data[4];
        let no_probability = data[6];
        let yes_probability = data[8];

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
        };

        Some(user_bet)
    } else {
        println!("Event data is too short.");
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
    sqlx::query(
        "INSERT INTO bet_placed (
            bet,
            amount,
            has_claimed,
            claimable_amount,
            no_probability,
            yes_probability,
            block_number,
            transaction_hash,
            \"from_address\"
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (block_number, transaction_hash) DO NOTHING",
    )
    .bind(event.bet)
    .bind(biguint_to_bigdecimal_scaled(&event.amount, 18)) 
    .bind(event.has_claimed)
    .bind(biguint_to_bigdecimal_scaled(&event.claimable_amount, 18)) 
    .bind(event.user_odds.no_probability as i64)
    .bind(event.user_odds.yes_probability as i64)
    .bind(block_number as i64)
    .bind(transaction_hash)
    .bind(from_address)
    .execute(pool)
    .await
    .expect("Failed to store event in database");
}
