CREATE TYPE role AS ENUM ('admin', 'user', 'super_admin');

CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    role role NOT NULL DEFAULT 'user',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    is_removed BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Named `UserTodo` in Rust (examples/table_naming.rs) to demonstrate that
-- the default table name is derived by splitting camel case before
-- pluralizing (`user_todo` + `s`), not by lowercasing the whole struct name
-- (`usertodos`).
CREATE TABLE user_todos (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id),
    todo_id INTEGER NOT NULL
);
