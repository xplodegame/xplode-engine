-- Rename network column to currency in game_pnl table
ALTER TABLE game_pnl RENAME COLUMN network TO currency;

-- Rename network column to currency in user_network_pnl table
ALTER TABLE user_network_pnl RENAME COLUMN network TO currency;

-- Drop existing indexes
DROP INDEX IF EXISTS idx_game_pnl_network_time;
DROP INDEX IF EXISTS idx_user_network_pnl_profit;

-- Create new indexes with currency column
CREATE INDEX idx_game_pnl_currency_time ON game_pnl(currency, created_at DESC);
CREATE INDEX idx_user_network_pnl_profit ON user_network_pnl(currency, total_profit DESC);

-- Drop existing views
DROP VIEW IF EXISTS leaderboard_24h;
DROP VIEW IF EXISTS leaderboard_all_time;

-- Recreate views with currency column
CREATE VIEW leaderboard_24h AS
SELECT 
    u.name,
    g.currency,
    COUNT(*) as total_matches,
    SUM(g.profit) as total_profit,
    RANK() OVER (PARTITION BY g.currency ORDER BY SUM(g.profit) DESC) as rank
FROM game_pnl g
JOIN users u ON g.user_id = u.id
WHERE g.created_at >= NOW() - INTERVAL '24 hours'
GROUP BY u.name, g.currency;

CREATE VIEW leaderboard_all_time AS
SELECT 
    u.name,
    p.currency,
    p.total_profit,
    p.total_matches,
    RANK() OVER (PARTITION BY p.currency ORDER BY p.total_profit DESC) as rank
FROM user_network_pnl p
JOIN users u ON p.user_id = u.id; 