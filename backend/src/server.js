/**
 * Stellarcade Backend - Main Express Server
 * 
 * This file initializes the Express application, mounts middleware, 
 * defines routes, and starts the server.
 */

const express = require('express');
const helmet = require('helmet');
const cors = require('cors');
const morgan = require('morgan');
require('dotenv').config();

// Config and Utils
const logger = require('./utils/logger');
const db = require('./config/database');
const redis = require('./config/redis');

// Middleware
const errorHandler = require('./middleware/errorHandler.middleware');

// Routes
const routes = require('./routes');

const app = express();

/**
 * Standard Security and Utility Middleware
 */
app.use(helmet()); // Basic security headers
app.use(cors()); // Enable Cross-Origin Resource Sharing
app.use(express.json()); // Body parser for JSON
app.use(morgan('combined', { stream: { write: message => logger.info(message.trim()) } })); // HTTP request logging

/**
 * API Route Mounting
 */
app.use('/api', routes);

/**
 * Health Check Endpoint
 * Assumes database and redis are healthy if the server is running.
 */
app.get('/api/health', (req, res) => {
  res.status(200).json({ 
    status: 'Operational', 
    timestamp: new Date().toISOString(),
    service: 'stellarcade-api'
  });
});

/**
 * Global Error Handling
 * MUST be the last middleware mounted.
 */
app.use(errorHandler);

/**
 * Server Lifecycle Management
 */
const PORT = process.env.PORT || 3000;
const server = app.listen(PORT, () => {
  logger.info(`🚀 Stellarcade Backend is live on port ${PORT}`);
  logger.info(`Environment: ${process.env.NODE_ENV || 'development'}`);
});

/**
 * Graceful Shutdown Logic
 */
const gracefulShutdown = async () => {
  logger.info('SIGTERM/SIGINT received. Starting graceful shutdown...');
  
  // Close the server first to stop accepting new requests
  server.close(async () => {
    logger.info('HTTP server closed.');
    
    try {
      // Close database and redis connections
      await db.destroy();
      logger.info('Database connection closed.');
      
      // Redis client disconnect
      if (redis.isOpen) {
        await redis.quit();
        logger.info('Redis connection closed.');
      }
      
      process.exit(0);
    } catch (err) {
      logger.error('Error during shutdown:', err);
      process.exit(1);
    }
  });
};

process.on('SIGTERM', gracefulShutdown);
process.on('SIGINT', gracefulShutdown);

module.exports = app;
