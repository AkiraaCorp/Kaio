<h1 align="center" style="font-size: 3em;">Kaio 📡</h1>

<div align="center">
  <img src="Kaio.png" alt="Kaio Logo" width="300"/>
</div>

**Kaio** is a Rust-based blockchain event listener and processor specifically designed to interact with the StarkNet blockchain. It listens for specific events emitted from smart contracts, parses them, and stores the results in a PostgreSQL database. Kaio is intended to help developers monitor and react to smart contract events on StarkNet, providing an efficient way to build real-time data pipelines for decentralized applications.

## Features

- Listens for `BetPlace` events on StarkNet smart contracts.
- Parses event data, including user address, bet amount, probabilities, and more.
- Automatically syncs the latest blocks and stores event data in a PostgreSQL database.
- Efficient handling of large event streams.
- Supports automatic reconnection and resynchronization.

## Requirements

Before you start, make sure you have the following installed:

- Rust (latest stable version)
- PostgreSQL (version 12 or higher)
- Docker (optional, if you want to use Docker for the database setup)

## Setup

### 1. Clone the Repository

```bash
git clone https://github.com/AkiraaCorp/kaio.git
cd kaio
```

## Configuration

- **RPC Endpoint:**  
  You can modify the RPC URL in the main function (`let rpc_url = Url::parse(...)`) to point to a different StarkNet endpoint if needed.

- **Contract Addresses:**  
  Update the `contract_addresses` vector with the addresses of the contracts you want to monitor.

## Running the Program

When you run **Kaio** using `cargo run`, it will:

1. **Connect to the Specified RPC Endpoint:**  
   Ensure that your specified RPC provider (e.g., [https://free-rpc.nethermind.io/sepolia-juno/](https://free-rpc.nethermind.io/sepolia-juno/)) is accessible (its free but you will be rate limit)

2. **Check the Database:**  
   If the database is not initialized, it will automatically create the required tables.

3. **Start Listening for Events:**  
   Kaio will start processing blocks from the last synced block and look for `BetPlace` events. Each event will be parsed and stored in the database.

4. **Log Events and Store Data:**  
   Successfully parsed events will be printed to the console and stored in the `bets` table in your PostgreSQL database.

## Manual Setup for Specific Use Cases

### Changing Contracts

If you want to listen to a different set of contracts, update the following section in the code:

```rust
let contract_addresses = vec![
    Felt::from_hex("CONTRACT_TO_LISTEN")
        .expect("Invalid contract address"),
];
```
Add or remove contracts as needed.

## Troubleshooting

### Database Connection Error:

- Ensure that your PostgreSQL instance is running and accessible at the specified `DATABASE_URL`.
- Check firewall or connection issues if you are using a remote PostgreSQL instance.

### Event Parsing Issues:

- If event data parsing fails, verify the contract ABI and event signature.
- You can print the raw event data using `println!` to debug the issue.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Feel free to submit issues or pull requests if you have any improvements or suggestions!

## Author

Kaio was developed by **[AkiraaCorp]**. Reach out at `contact@sightbet.com` for inquiries.

