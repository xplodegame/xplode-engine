-- Add gif_ids column to users table
ALTER TABLE users
ADD COLUMN gif_ids INTEGER[] DEFAULT '{}';

-- Create an index for faster array operations
CREATE INDEX idx_users_gif_ids ON users USING GIN (gif_ids); 