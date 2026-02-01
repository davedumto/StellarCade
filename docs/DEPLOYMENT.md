# Deployment Guide

This document covers the deployment process for Stellarcade smart contracts and the backend API.

## ⛓ Smart Contracts

### Testnet Deployment

1. Build all contracts:
   ```bash
   ./scripts/deploy-contracts.sh --build
   ```
2. Deploy to Futurenet/Testnet:
   ```bash
   soroban contract deploy --wasm target/wasm32-unknown-unknown/release/prize_pool.wasm --source <YOUR_IDENTITY> --network testnet
   ```
3. Save the returned Contract IDs to your `.env` file in the backend.

### Mainnet Deployment

- [ ] Prepare audited WASM binaries.
- [ ] Ensure the admin identity is a secure multi-sig account.
- [ ] Deploy using the same Soroban CLI commands with `--network mainnet`.

---

## 🚀 Backend API

### Docker Deployment

1. Build the production image:
   ```bash
   docker build -t stellarcade-backend ./backend
   ```
2. Deploy using your preferred orchestration tool (Kubernetes, AWS ECS, etc.).

### Environment Setup

Ensure the following variables are set in your production environment:

- `NODE_ENV=production`
- `DATABASE_URL`
- `REDIS_URL`
- `STELLAR_NETWORK=public`
- `HORIZON_URL=https://horizon.stellar.org`

## 🔄 CI/CD Pipeline

We use GitHub Actions for our CI/CD pipeline.

- **Lint & Test**: Triggered on every Pull Request.
- **Build & Deploy**: Triggered on every merge to `main`.

## 📈 Monitoring

- Use **Winston** for structured logging.
- Integrate with **Sentry** for error tracking.
- Monitor Stellar network status via the [Stellar Status Dashboard](https://status.stellar.org/).

## 🔙 Rollback Procedures

- **Backend**: Redeploy the previous Docker image tag.
- **Contracts**: Update the contract reference in the backend to the previous stable Contract ID (Note: On-chain contracts are typically immutable or require a migration).
