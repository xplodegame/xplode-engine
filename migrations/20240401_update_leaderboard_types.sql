-- Drop existing views
DROP VIEW IF EXISTS leaderboard_24h;
DROP VIEW IF EXISTS leaderboard_all_time;

-- Clear query plan cache
DISCARD ALL;

-- Create views with temporary names
CREATE VIEW leaderboard_24h_new AS
SELECT 
    u.name,
    g.currency,
    COUNT(*)::INT8 as total_matches,
    SUM(g.profit)::FLOAT8 as total_profit,
    RANK() OVER (PARTITION BY g.currency ORDER BY SUM(g.profit) DESC)::INT8 as rank
FROM game_pnl g
JOIN users u ON g.user_id = u.id
WHERE g.created_at >= NOW() - INTERVAL '24 hours'
GROUP BY u.name, g.currency;

CREATE VIEW leaderboard_all_time_new AS
SELECT 
    u.name,
    p.currency,
    p.total_profit::FLOAT8,
    p.total_matches::INT8,
    RANK() OVER (PARTITION BY p.currency ORDER BY p.total_profit DESC)::INT8 as rank
FROM user_network_pnl p
JOIN users u ON p.user_id = u.id;

-- Rename views to their final names
ALTER VIEW leaderboard_24h_new RENAME TO leaderboard_24h;
ALTER VIEW leaderboard_all_time_new RENAME TO leaderboard_all_time; 