# Stellarcade Backend

## 🚀 Overview

The Stellarcade Backend is a Node.js Express application that manages game logic, user accounts, and interactions with the Stellar network.

## 🛠 Tech Stack

- **Framework**: Express.js
- **Database**: PostgreSQL (via Knex.js)
- **Cache**: Redis
- **Blockchain**: Stellar SDK
- **Logging**: Winston
- **Validation**: Express Validator
- **Testing**: Jest & Supertest

## 📂 Folder Structure

- `src/config/`: Configuration for DB, Redis, Stellar, and Logger.
- `src/controllers/`: Route handlers.
- `src/services/`: Business logic and blockchain interactions.
- `src/models/`: Database models.
- `src/routes/`: API endpoint definitions.
- `src/middleware/`: Custom middleware (auth, error handling, validation).
- `migrations/`: Knex database migrations.
- `tests/`: Test suites.

## 🚦 Getting Started

1. Install dependencies:

   ```bash
   npm install
   ```

2. Configure environment:

   ```bash
   cp .env.example .env
   # Update variables
   ```

3. Run migrations:

   ```bash
   npm run migrate
   ```

4. Start development server:
   ```bash
   npm run dev
   ```

## 🧪 Testing

Run all tests:

```bash
npm test
```

---

_Built with ❤️ for the Stellarcade community._
