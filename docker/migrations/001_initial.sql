CREATE TABLE players (
    id            BIGSERIAL PRIMARY KEY,
    username      VARCHAR(32) UNIQUE NOT NULL,
    password_hash VARCHAR(128) NOT NULL,
    created_at    TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE player_characters (
    id           BIGSERIAL PRIMARY KEY,
    player_id    BIGINT REFERENCES players(id) NOT NULL,
    name         VARCHAR(32) NOT NULL,
    level        INT DEFAULT 1,
    experience   BIGINT DEFAULT 0,
    hp           INT DEFAULT 200,
    max_hp       INT DEFAULT 200,
    position_x   REAL DEFAULT 0.0,
    position_y   REAL DEFAULT 0.0,
    position_map VARCHAR(32) DEFAULT 'starter',
    stats        JSONB DEFAULT '{"str":10,"dex":10,"int":10}'::jsonb,
    updated_at   TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE player_inventory (
    character_id BIGINT REFERENCES player_characters(id),
    slot         SMALLINT,
    item_data    JSONB NOT NULL,
    PRIMARY KEY (character_id, slot)
);

-- trigger updated_at
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_player_characters_updated_at
  BEFORE UPDATE ON player_characters
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();
