CREATE TABLE ai_models (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    credit_cost_per_page INTEGER NOT NULL DEFAULT 1,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed default models
INSERT INTO ai_models (id, display_name, credit_cost_per_page, is_active) VALUES
    ('gemini-2.5-flash','Gemini 2.5 Flash',1, true);
