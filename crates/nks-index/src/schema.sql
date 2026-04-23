CREATE TABLE IF NOT EXISTS presets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    path        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL DEFAULT '',
    vendor      TEXT NOT NULL DEFAULT '',
    author      TEXT NOT NULL DEFAULT '',
    comment     TEXT NOT NULL DEFAULT '',
    plugin_ref  TEXT NOT NULL DEFAULT '',
    bank_chain  TEXT NOT NULL DEFAULT '',
    size        INTEGER NOT NULL DEFAULT 0,
    mtime_ns    INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_presets_plugin ON presets(plugin_ref);
CREATE INDEX IF NOT EXISTS idx_presets_vendor ON presets(vendor);

CREATE TABLE IF NOT EXISTS preset_types (
    preset_id INTEGER NOT NULL REFERENCES presets(id) ON DELETE CASCADE,
    type      TEXT NOT NULL,
    subtype   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_preset_types_type ON preset_types(type);

CREATE TABLE IF NOT EXISTS preset_modes (
    preset_id INTEGER NOT NULL REFERENCES presets(id) ON DELETE CASCADE,
    mode      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_preset_modes_mode ON preset_modes(mode);

CREATE TABLE IF NOT EXISTS favorites (
    preset_id INTEGER PRIMARY KEY REFERENCES presets(id) ON DELETE CASCADE,
    added_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS recents (
    preset_id INTEGER PRIMARY KEY REFERENCES presets(id) ON DELETE CASCADE,
    last_used INTEGER NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS presets_fts USING fts5(
    name, vendor, author, comment, bank_chain,
    content='',
    tokenize='porter unicode61'
);
