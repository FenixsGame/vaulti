CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  expires_at INTEGER NOT NULL
);
CREATE TABLE folders (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  parent_id TEXT REFERENCES folders(id),
  parent_key TEXT NOT NULL GENERATED ALWAYS AS (COALESCE(parent_id, '!root!')),
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (user_id, parent_key, name)
);
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  folder_id TEXT REFERENCES folders(id),
  folder_key TEXT NOT NULL GENERATED ALWAYS AS (COALESCE(folder_id, '!root!')),
  name TEXT NOT NULL,
  size INTEGER NOT NULL,
  mime TEXT NOT NULL,
  stored_name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  deleted_at INTEGER,
  UNIQUE (user_id, folder_key, name)
);
CREATE TABLE uploads (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  chunk_size INTEGER NOT NULL,
  total_chunks INTEGER NOT NULL,
  size INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE upload_chunks (
  upload_id TEXT NOT NULL REFERENCES uploads(id),
  chunk_n INTEGER NOT NULL,
  len INTEGER NOT NULL,
  received_at INTEGER NOT NULL,
  PRIMARY KEY (upload_id, chunk_n)
);
CREATE INDEX idx_files_user ON files(user_id, deleted_at);
