CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    username text NOT NULL UNIQUE,
    name text NOT NULL,
    password_hash text NOT NULL
);
CREATE TABLE IF NOT EXISTS organizations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL
);
CREATE TABLE IF NOT EXISTS members (
    org_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('owner','admin','member')),
    guide_dismissed boolean NOT NULL DEFAULT false,
    PRIMARY KEY (org_id, user_id)
);
CREATE TABLE IF NOT EXISTS sessions (
    digest text PRIMARY KEY,
    org_id uuid NOT NULL,
    user_id uuid NOT NULL,
    expires_at timestamptz NOT NULL DEFAULT (now() + interval '30 days'),
    FOREIGN KEY (org_id,user_id) REFERENCES members(org_id,user_id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS invitations (
    digest text PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('admin','member')),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz
);
CREATE TABLE IF NOT EXISTS spaces (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name text NOT NULL,
    description text NOT NULL DEFAULT '',
    next_task_number integer NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id,name)
);
CREATE TABLE IF NOT EXISTS machines (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    owner_id uuid NOT NULL REFERENCES users(id),
    name text NOT NULL,
    worker_digest text NOT NULL UNIQUE,
    key_digest text,
    shared boolean NOT NULL DEFAULT false,
    runtimes jsonb NOT NULL DEFAULT '[]'::jsonb,
    capacity integer NOT NULL DEFAULT 1 CHECK (capacity BETWEEN 1 AND 64),
    last_seen timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS agents (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id uuid NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    instructions text NOT NULL DEFAULT '',
    machine_id uuid NOT NULL REFERENCES machines(id),
    runtime text NOT NULL,
    owner_id uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (space_id,name)
);
CREATE TABLE IF NOT EXISTS iterations (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id uuid NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    starts_on date NOT NULL,
    ends_on date NOT NULL CHECK (ends_on >= starts_on),
    status text NOT NULL DEFAULT 'planned' CHECK (status IN ('planned','active','closed'))
);
CREATE TABLE IF NOT EXISTS tasks (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id uuid NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    number integer NOT NULL,
    title text NOT NULL,
    description text NOT NULL DEFAULT '',
    status text NOT NULL CHECK (status IN ('backlog','todo','doing','review','done')),
    priority text NOT NULL DEFAULT 'none' CHECK (priority IN ('none','low','medium','high','urgent')),
    type text NOT NULL DEFAULT 'task' CHECK (type IN ('task','feature','bug')),
    member_id uuid REFERENCES users(id),
    agent_id uuid REFERENCES agents(id),
    iteration_id uuid REFERENCES iterations(id),
    due_on date,
    created_by uuid NOT NULL REFERENCES users(id),
    revision integer NOT NULL DEFAULT 1,
    updated_at timestamptz NOT NULL DEFAULT now(),
    archived_at timestamptz,
    UNIQUE (space_id,number),
    CHECK (member_id IS NULL OR agent_id IS NULL)
);
CREATE TABLE IF NOT EXISTS comments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author_id uuid NOT NULL REFERENCES users(id),
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS documents (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id uuid NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    title text NOT NULL,
    body text NOT NULL,
    revision integer NOT NULL DEFAULT 1,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS document_versions (
    document_id uuid NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    revision integer NOT NULL,
    title text NOT NULL,
    body text NOT NULL,
    author_id uuid NOT NULL REFERENCES users(id),
    saved_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (document_id,revision)
);
CREATE TABLE IF NOT EXISTS chats (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    space_id uuid NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    owner_id uuid NOT NULL REFERENCES users(id),
    agent_id uuid NOT NULL REFERENCES agents(id),
    task_id uuid REFERENCES tasks(id) ON DELETE CASCADE,
    title text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (task_id,agent_id)
);
CREATE TABLE IF NOT EXISTS runs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id uuid REFERENCES tasks(id) ON DELETE CASCADE,
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    agent_id uuid NOT NULL REFERENCES agents(id),
    machine_id uuid NOT NULL REFERENCES machines(id),
    requested_by uuid NOT NULL REFERENCES users(id),
    instruction text NOT NULL,
    status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','canceling','blocked','done','failed','canceled')),
    lease uuid,
    lease_until timestamptz,
    report_digest text,
    detail text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS runs_one_active_chat ON runs(chat_id)
    WHERE status IN ('queued','running','canceling');
CREATE INDEX IF NOT EXISTS runs_machine_queue ON runs(machine_id,created_at) WHERE status='queued';
CREATE TABLE IF NOT EXISTS messages (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id uuid NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    run_id uuid REFERENCES runs(id) ON DELETE SET NULL,
    sender text NOT NULL,
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS notifications (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    task_id uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    title text NOT NULL,
    opened_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
