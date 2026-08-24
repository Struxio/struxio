-- SPDX-License-Identifier: AGPL-3.0-only
-- Deterministic local workspace for single-tenant OSS composition.
-- workspace id = UUID v5(DNS, 'local.struxio')
-- principal id = UUID v5(DNS, 'local.struxio.operator')
-- Neither identifier is the nil UUID.

CREATE TABLE workspaces (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT workspaces_id_not_nil CHECK (id <> '00000000-0000-0000-0000-000000000000')
);

CREATE TABLE principals (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    subject TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT principals_id_not_nil CHECK (id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT principals_provider_subject_key UNIQUE (provider, subject)
);

CREATE TABLE workspace_memberships (
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    principal_id UUID NOT NULL REFERENCES principals (id) ON DELETE CASCADE,
    scopes TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, principal_id),
    CONSTRAINT workspace_memberships_workspace_id_not_nil
        CHECK (workspace_id <> '00000000-0000-0000-0000-000000000000'),
    CONSTRAINT workspace_memberships_principal_id_not_nil
        CHECK (principal_id <> '00000000-0000-0000-0000-000000000000')
);

INSERT INTO workspaces (id, slug, name)
VALUES (
    '79ca2631-6230-5277-9d0e-879be839f744',
    'local',
    'Local workspace'
);

INSERT INTO principals (id, provider, subject)
VALUES (
    '271a2afd-e58f-5f5b-9c32-57c0f38df33e',
    'local',
    'operator'
);

INSERT INTO workspace_memberships (workspace_id, principal_id, scopes)
VALUES (
    '79ca2631-6230-5277-9d0e-879be839f744',
    '271a2afd-e58f-5f5b-9c32-57c0f38df33e',
    ARRAY[
        'documents:read',
        'documents:write',
        'templates:read',
        'templates:write',
        'extractions:create',
        'extractions:read',
        'batches:create',
        'batches:read'
    ]::TEXT[]
);
