# Setup Guide

Follow these steps to set up Stellarcade for local development.

## 📋 Prerequisites

- **Rust**: [Install Rust](https://www.rust-lang.org/tools/install)
- **Soroban CLI**: `cargo install --locked soroban-cli`
- **Node.js**: [Install Node.js v18+](https://nodejs.org/)
- **Docker**: [Install Docker Desktop](https://www.docker.com/products/docker-desktop)
- **Stellar Wallet**: Use a testnet account (e.g., via [Laboratory](https://laboratory.stellar.org/))

## 📂 Installation

1. Clone the repository:

   ```bash
   git clone https://github.com/stellar/stellarcade.git
   cd stellarcade
   ```

2. Install Backend dependencies:

   ```bash
   cd backend
   npm install
   ```

3. Setup environment variables:
   ```bash
   cp .env.example .env
   # Edit .env with your local settings
   ```

## ⛓ Smart Contract Setup

1. Navigate to a contract folder:

   ```bash
   cd contracts/prize-pool
   ```

2. Build the contract:

   ```bash
   cargo build --target wasm32-unknown-unknown --release
   ```

3. Run tests:
   ```bash
   cargo test
   ```

## 🐳 Running with Docker

The easiest way to get the backend, database, and Redis running is via Docker Compose.

```bash
docker-compose up -d
```

## 🧪 Seeding the Database

To populate your local database with sample data:

```bash
cd backend
npm run seed
```

## 🛠 Troubleshooting

- **Build Errors**: Ensure your Rust version is up to date (`rustup update`).
- **Database Connection**: Verify PostgreSQL is running in Docker (`docker ps`).
- **Contract Deployment**: Ensure your testnet account has sufficient XLM (use Friendbot).
