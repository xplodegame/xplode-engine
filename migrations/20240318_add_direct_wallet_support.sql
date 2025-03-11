-- Add support for direct wallets and improve transaction tracking

-- Make user_pda optional
ALTER TABLE users 
ALTER COLUMN user_pda DROP NOT NULL;

-- Add wallet type and address to wallet table
ALTER TABLE wallet
ADD COLUMN wallet_type TEXT NOT NULL DEFAULT 'PDA',
ADD COLUMN wallet_address TEXT;

-- Enhance transactions table
ALTER TABLE transactions
ADD COLUMN wallet_id INTEGER REFERENCES wallet(id),
ADD COLUMN status TEXT NOT NULL DEFAULT 'pending',
ADD COLUMN network TEXT NOT NULL DEFAULT 'solana';

-- Add index for faster wallet-based transaction queries
CREATE INDEX idx_transactions_wallet_id ON transactions(wallet_id);

-- Migrate existing transactions to link with wallets
UPDATE transactions t
SET wallet_id = w.id
FROM wallet w
WHERE t.user_id = w.user_id 
AND t.currency = w.currency;

-- Rename existing pnl table to game_pnl and modify
ALTER TABLE pnl RENAME TO game_pnl;
ALTER TABLE game_pnl
ADD COLUMN network TEXT NOT NULL DEFAULT 'solana',
DROP COLUMN num_matches; -- Remove this as it's redundant

-- Create new table for overall user network PNL
CREATE TABLE user_network_pnl (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    network TEXT NOT NULL,
    total_matches INTEGER NOT NULL DEFAULT 0,
    total_profit DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (user_id, network)  -- Keep this constraint as it's important for data integrity
);

-- Create indexes for efficient queries
CREATE INDEX idx_game_pnl_network_time ON game_pnl(network, created_at DESC);
CREATE INDEX idx_user_network_pnl_profit ON user_network_pnl(network, total_profit DESC);

-- Create view for 24h leaderboard
CREATE VIEW leaderboard_24h AS
SELECT 
    u.name,
    g.network,
    COUNT(*) as games_played,
    SUM(g.profit) as total_profit,
    RANK() OVER (PARTITION BY g.network ORDER BY SUM(g.profit) DESC) as rank
FROM game_pnl g
JOIN users u ON g.user_id = u.id
WHERE g.created_at >= NOW() - INTERVAL '24 hours'
GROUP BY u.name, g.network;

-- Create view for all-time leaderboard
CREATE VIEW leaderboard_all_time AS
SELECT 
    u.name,
    p.network,
    p.total_matches as games_played,
    p.total_profit,
    RANK() OVER (PARTITION BY p.network ORDER BY p.total_profit DESC) as rank
FROM user_network_pnl p
JOIN users u ON p.user_id = u.id;
