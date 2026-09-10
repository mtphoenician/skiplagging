use sqlx::{PgConnection, PgPool};

const SCHEMA_LOCK: i64 = 872_341_001;

const ALTERS: &[&str] = &[
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS continent VARCHAR(8) DEFAULT ''",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS country_name TEXT DEFAULT ''",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS region_name TEXT DEFAULT ''",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS home_link TEXT",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS gps_code VARCHAR(16)",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS local_code VARCHAR(16)",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS keywords TEXT",
    "ALTER TABLE hidden_deals ALTER COLUMN bookers SET DEFAULT '[]'::jsonb",
    "UPDATE hidden_deals SET bookers = '[]'::jsonb WHERE bookers IS NULL",
];

pub async fn init_db(pool: &PgPool) -> anyhow::Result<()> {
    let mut conn = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(SCHEMA_LOCK)
        .execute(&mut *conn)
        .await?;
    let result = init_db_locked(&mut *conn).await;
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(SCHEMA_LOCK)
        .execute(&mut *conn)
        .await;
    result
}

async fn init_db_locked(conn: &mut PgConnection) -> anyhow::Result<()> {
    for stmt in CREATE_TABLES {
        sqlx::query(stmt).execute(&mut *conn).await?;
    }
    for stmt in ALTERS {
        sqlx::query(stmt).execute(&mut *conn).await?;
    }
    for ext in [
        "CREATE EXTENSION IF NOT EXISTS unaccent",
        "CREATE EXTENSION IF NOT EXISTS pg_trgm",
    ] {
        let _ = sqlx::query(ext).execute(&mut *conn).await;
    }
    Ok(())
}

const CREATE_TABLES: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS continents (
        code VARCHAR(2) PRIMARY KEY,
        name TEXT NOT NULL,
        source VARCHAR(32) NOT NULL DEFAULT 'ourairports'
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS countries (
        iso2 VARCHAR(2) PRIMARY KEY,
        iso3 VARCHAR(3),
        iso_numeric VARCHAR(8),
        name TEXT NOT NULL,
        continent VARCHAR(2) NOT NULL DEFAULT '',
        capital TEXT NOT NULL DEFAULT '',
        currency_code VARCHAR(8) NOT NULL DEFAULT '',
        currency_name TEXT NOT NULL DEFAULT '',
        tld VARCHAR(16) NOT NULL DEFAULT '',
        phone VARCHAR(32) NOT NULL DEFAULT '',
        languages TEXT NOT NULL DEFAULT '',
        population INTEGER,
        area_km2 DOUBLE PRECISION,
        geoname_id INTEGER,
        wikipedia TEXT,
        sources TEXT NOT NULL DEFAULT 'ourairports',
        ingested_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_countries_iso3 ON countries (iso3)",
    "CREATE INDEX IF NOT EXISTS ix_countries_name ON countries (name)",
    "CREATE INDEX IF NOT EXISTS ix_countries_continent ON countries (continent)",
    r#"
    CREATE TABLE IF NOT EXISTS regions (
        code VARCHAR(16) PRIMARY KEY,
        local_code VARCHAR(16) NOT NULL DEFAULT '',
        name TEXT NOT NULL,
        iso_country VARCHAR(2) NOT NULL,
        continent VARCHAR(2) NOT NULL DEFAULT '',
        wikipedia TEXT,
        source VARCHAR(32) NOT NULL DEFAULT 'ourairports'
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_regions_name ON regions (name)",
    "CREATE INDEX IF NOT EXISTS ix_regions_iso_country ON regions (iso_country)",
    r#"
    CREATE TABLE IF NOT EXISTS airports (
        iata VARCHAR(3) PRIMARY KEY,
        icao VARCHAR(8),
        ident VARCHAR(16),
        name TEXT NOT NULL,
        municipality TEXT NOT NULL DEFAULT '',
        iso_country VARCHAR(8) NOT NULL DEFAULT '',
        iso_region VARCHAR(16) NOT NULL DEFAULT '',
        continent VARCHAR(8) NOT NULL DEFAULT '',
        country_name TEXT NOT NULL DEFAULT '',
        region_name TEXT NOT NULL DEFAULT '',
        lat DOUBLE PRECISION NOT NULL,
        lon DOUBLE PRECISION NOT NULL,
        elevation_ft INTEGER,
        "type" VARCHAR(32) NOT NULL DEFAULT 'large_airport',
        scheduled_service BOOLEAN NOT NULL DEFAULT TRUE,
        wikipedia TEXT,
        home_link TEXT,
        gps_code VARCHAR(16),
        local_code VARCHAR(16),
        keywords TEXT,
        source VARCHAR(32) NOT NULL DEFAULT 'ourairports',
        ingested_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_airports_icao ON airports (icao)",
    "CREATE INDEX IF NOT EXISTS ix_airports_ident ON airports (ident)",
    "CREATE INDEX IF NOT EXISTS ix_airports_municipality ON airports (municipality)",
    "CREATE INDEX IF NOT EXISTS ix_airports_iso_country ON airports (iso_country)",
    "CREATE INDEX IF NOT EXISTS ix_airports_iso_region ON airports (iso_region)",
    "CREATE INDEX IF NOT EXISTS ix_airports_country_name ON airports (country_name)",
    r#"
    CREATE TABLE IF NOT EXISTS runways (
        id INTEGER PRIMARY KEY,
        airport_iata VARCHAR(3) NOT NULL,
        airport_ident VARCHAR(16) NOT NULL,
        length_ft INTEGER,
        width_ft INTEGER,
        surface VARCHAR(64) NOT NULL DEFAULT '',
        lighted BOOLEAN NOT NULL DEFAULT FALSE,
        closed BOOLEAN NOT NULL DEFAULT FALSE,
        le_ident VARCHAR(16) NOT NULL DEFAULT '',
        he_ident VARCHAR(16) NOT NULL DEFAULT '',
        source VARCHAR(32) NOT NULL DEFAULT 'ourairports'
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_runways_airport_iata ON runways (airport_iata)",
    "CREATE INDEX IF NOT EXISTS ix_runways_airport_ident ON runways (airport_ident)",
    r#"
    CREATE TABLE IF NOT EXISTS navaids (
        id INTEGER PRIMARY KEY,
        ident VARCHAR(16) NOT NULL,
        name TEXT NOT NULL,
        "type" VARCHAR(16) NOT NULL DEFAULT '',
        frequency_khz INTEGER,
        iso_country VARCHAR(2) NOT NULL DEFAULT '',
        associated_airport VARCHAR(16),
        lat DOUBLE PRECISION,
        lon DOUBLE PRECISION,
        source VARCHAR(32) NOT NULL DEFAULT 'ourairports'
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_navaids_ident ON navaids (ident)",
    "CREATE INDEX IF NOT EXISTS ix_navaids_iso_country ON navaids (iso_country)",
    "CREATE INDEX IF NOT EXISTS ix_navaids_associated_airport ON navaids (associated_airport)",
    r#"
    CREATE TABLE IF NOT EXISTS airlines (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        iata VARCHAR(8),
        icao VARCHAR(8),
        name TEXT NOT NULL,
        country TEXT NOT NULL DEFAULT '',
        active BOOLEAN NOT NULL DEFAULT TRUE,
        source VARCHAR(32) NOT NULL DEFAULT 'openflights',
        source_note TEXT NOT NULL DEFAULT ''
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_airlines_iata ON airlines (iata)",
    "CREATE INDEX IF NOT EXISTS ix_airlines_icao ON airlines (icao)",
    r#"
    CREATE TABLE IF NOT EXISTS routes (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        airline_iata VARCHAR(8) NOT NULL,
        origin_iata VARCHAR(3) NOT NULL,
        dest_iata VARCHAR(3) NOT NULL,
        codeshare BOOLEAN NOT NULL DEFAULT FALSE,
        stops INTEGER NOT NULL DEFAULT 0,
        equipment VARCHAR(64) NOT NULL DEFAULT '',
        source VARCHAR(32) NOT NULL DEFAULT 'openflights',
        source_note TEXT NOT NULL DEFAULT '',
        CONSTRAINT uq_route UNIQUE (airline_iata, origin_iata, dest_iata)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_routes_airline_iata ON routes (airline_iata)",
    "CREATE INDEX IF NOT EXISTS ix_routes_origin_iata ON routes (origin_iata)",
    "CREATE INDEX IF NOT EXISTS ix_routes_dest_iata ON routes (dest_iata)",
    r#"
    CREATE TABLE IF NOT EXISTS searches (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        origin VARCHAR(3) NOT NULL,
        destination VARCHAR(3) NOT NULL,
        date VARCHAR(10) NOT NULL,
        adults INTEGER NOT NULL DEFAULT 1,
        cabin VARCHAR(24) NOT NULL DEFAULT 'ECONOMY',
        currency VARCHAR(8) NOT NULL DEFAULT 'USD',
        elapsed_ms DOUBLE PRECISION NOT NULL DEFAULT 0,
        sources JSONB NOT NULL DEFAULT '[]'::jsonb,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_searches_origin ON searches (origin)",
    "CREATE INDEX IF NOT EXISTS ix_searches_destination ON searches (destination)",
    "CREATE INDEX IF NOT EXISTS ix_searches_date ON searches (date)",
    r#"
    CREATE TABLE IF NOT EXISTS offers (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        search_id INTEGER NOT NULL REFERENCES searches(id) ON DELETE CASCADE,
        offer_uid VARCHAR(80) NOT NULL,
        source VARCHAR(32) NOT NULL,
        layer VARCHAR(24) NOT NULL,
        kind VARCHAR(24) NOT NULL,
        price DOUBLE PRECISION,
        currency VARCHAR(8),
        payload JSONB NOT NULL DEFAULT '{}'::jsonb,
        retrieved_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_offers_search_id ON offers (search_id)",
    "CREATE INDEX IF NOT EXISTS ix_offers_offer_uid ON offers (offer_uid)",
    r#"
    CREATE TABLE IF NOT EXISTS offer_observations (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        origin VARCHAR(3) NOT NULL,
        ticketed VARCHAR(3) NOT NULL,
        connections JSONB NOT NULL DEFAULT '[]'::jsonb,
        date VARCHAR(10) NOT NULL,
        adults INTEGER NOT NULL DEFAULT 1,
        cabin VARCHAR(24) NOT NULL DEFAULT 'ECONOMY',
        currency VARCHAR(8) NOT NULL DEFAULT 'USD',
        source VARCHAR(32) NOT NULL DEFAULT '',
        fingerprint VARCHAR(240) NOT NULL,
        price DOUBLE PRECISION NOT NULL,
        payload JSONB NOT NULL DEFAULT '{}'::jsonb,
        observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        CONSTRAINT uq_offer_obs UNIQUE (fingerprint, date, adults, cabin, source)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_offer_observations_origin ON offer_observations (origin)",
    "CREATE INDEX IF NOT EXISTS ix_offer_observations_ticketed ON offer_observations (ticketed)",
    "CREATE INDEX IF NOT EXISTS ix_offer_observations_date ON offer_observations (date)",
    "CREATE INDEX IF NOT EXISTS ix_offer_observations_fingerprint ON offer_observations (fingerprint)",
    "CREATE INDEX IF NOT EXISTS ix_offer_obs_lookup ON offer_observations (origin, date, adults, cabin, observed_at)",
    r#"
    CREATE TABLE IF NOT EXISTS fare_observations (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        offer_uid VARCHAR(80) NOT NULL,
        fingerprint VARCHAR(240) NOT NULL,
        origin VARCHAR(3) NOT NULL,
        ticketed VARCHAR(3) NOT NULL,
        connections JSONB NOT NULL DEFAULT '[]'::jsonb,
        stops INTEGER NOT NULL DEFAULT 0,
        carrier VARCHAR(8) NOT NULL DEFAULT '',
        provider VARCHAR(32) NOT NULL,
        price DOUBLE PRECISION NOT NULL,
        currency VARCHAR(8) NOT NULL DEFAULT 'USD',
        date VARCHAR(10) NOT NULL,
        adults INTEGER NOT NULL DEFAULT 1,
        cabin VARCHAR(24) NOT NULL DEFAULT 'ECONOMY',
        fare_brand VARCHAR(64),
        expires_at VARCHAR(40),
        observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_fare_observations_offer_uid ON fare_observations (offer_uid)",
    "CREATE INDEX IF NOT EXISTS ix_fare_observations_provider ON fare_observations (provider)",
    "CREATE INDEX IF NOT EXISTS ix_fare_obs_itin ON fare_observations (fingerprint, observed_at)",
    "CREATE INDEX IF NOT EXISTS ix_fare_obs_pair ON fare_observations (origin, ticketed, date)",
    r#"
    CREATE TABLE IF NOT EXISTS route_edges (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        origin VARCHAR(3) NOT NULL,
        dest VARCHAR(3) NOT NULL,
        carrier VARCHAR(8) NOT NULL DEFAULT '',
        flight_number VARCHAR(16) NOT NULL DEFAULT '',
        observation_count INTEGER NOT NULL DEFAULT 0,
        days_seen INTEGER NOT NULL DEFAULT 0,
        travel_dates JSONB NOT NULL DEFAULT '[]'::jsonb,
        first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        CONSTRAINT uq_route_edge UNIQUE (origin, dest, carrier, flight_number)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_route_edge_pair ON route_edges (origin, dest)",
    r#"
    CREATE TABLE IF NOT EXISTS hidden_city_route_stats (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        origin VARCHAR(3) NOT NULL,
        intended VARCHAR(3) NOT NULL,
        ticketed VARCHAR(3) NOT NULL,
        observations INTEGER NOT NULL DEFAULT 0,
        successful_connections INTEGER NOT NULL DEFAULT 0,
        cheaper_than_direct_count INTEGER NOT NULL DEFAULT 0,
        success_rate DOUBLE PRECISION NOT NULL DEFAULT 0,
        average_saving DOUBLE PRECISION NOT NULL DEFAULT 0,
        median_saving DOUBLE PRECISION NOT NULL DEFAULT 0,
        maximum_saving DOUBLE PRECISION NOT NULL DEFAULT 0,
        average_saving_percent DOUBLE PRECISION NOT NULL DEFAULT 0,
        savings JSONB NOT NULL DEFAULT '[]'::jsonb,
        currency VARCHAR(8) NOT NULL DEFAULT '',
        best_through_price DOUBLE PRECISION,
        score DOUBLE PRECISION NOT NULL DEFAULT 0,
        last_success_at TIMESTAMPTZ,
        last_cheaper_at TIMESTAMPTZ,
        last_checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        CONSTRAINT uq_hc_route_stat UNIQUE (origin, intended, ticketed)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_hidden_city_route_stats_score ON hidden_city_route_stats (score)",
    "CREATE INDEX IF NOT EXISTS ix_hc_stat_lookup ON hidden_city_route_stats (origin, intended, score)",
    r#"
    CREATE TABLE IF NOT EXISTS provider_calls (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        provider VARCHAR(32) NOT NULL,
        origin VARCHAR(3) NOT NULL,
        dest VARCHAR(3) NOT NULL,
        date VARCHAR(10) NOT NULL DEFAULT '',
        purpose VARCHAR(16) NOT NULL DEFAULT 'direct',
        ok BOOLEAN NOT NULL DEFAULT TRUE,
        offers INTEGER NOT NULL DEFAULT 0,
        latency_ms DOUBLE PRECISION NOT NULL DEFAULT 0,
        cost_usd DOUBLE PRECISION NOT NULL DEFAULT 0,
        called_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_provider_call_route ON provider_calls (provider, origin, dest)",
    "CREATE INDEX IF NOT EXISTS ix_provider_calls_called_at ON provider_calls (called_at)",
    r#"
    CREATE TABLE IF NOT EXISTS hidden_deals (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        origin VARCHAR(3) NOT NULL,
        destination VARCHAR(3) NOT NULL,
        hidden_city VARCHAR(3) NOT NULL,
        origin_city TEXT NOT NULL DEFAULT '',
        dest_city TEXT NOT NULL DEFAULT '',
        hidden_city_name TEXT NOT NULL DEFAULT '',
        date VARCHAR(10) NOT NULL,
        honest_price DOUBLE PRECISION NOT NULL,
        through_price DOUBLE PRECISION NOT NULL,
        currency VARCHAR(8) NOT NULL DEFAULT 'USD',
        saving DOUBLE PRECISION NOT NULL DEFAULT 0,
        saving_pct DOUBLE PRECISION NOT NULL DEFAULT 0,
        first_flight VARCHAR(16) NOT NULL DEFAULT '',
        source VARCHAR(32) NOT NULL DEFAULT '',
        bookers JSONB NOT NULL DEFAULT '[]'::jsonb,
        local_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
        through_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
        retrieved_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        CONSTRAINT uq_hidden_deal UNIQUE (origin, destination, hidden_city, date, first_flight)
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_hidden_deals_origin ON hidden_deals (origin)",
    "CREATE INDEX IF NOT EXISTS ix_hidden_deals_destination ON hidden_deals (destination)",
    "CREATE INDEX IF NOT EXISTS ix_hidden_deals_hidden_city ON hidden_deals (hidden_city)",
    "CREATE INDEX IF NOT EXISTS ix_hidden_deals_date ON hidden_deals (date)",
    r#"
    CREATE TABLE IF NOT EXISTS track_snapshots (
        id INTEGER PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
        airport_iata VARCHAR(3) NOT NULL,
        icao24 VARCHAR(16) NOT NULL,
        callsign VARCHAR(16),
        origin_country VARCHAR(64),
        lat DOUBLE PRECISION,
        lon DOUBLE PRECISION,
        baro_altitude_m DOUBLE PRECISION,
        on_ground BOOLEAN NOT NULL DEFAULT FALSE,
        velocity_ms DOUBLE PRECISION,
        true_track DOUBLE PRECISION,
        position_source INTEGER,
        api_time INTEGER,
        observed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
    "#,
    "CREATE INDEX IF NOT EXISTS ix_track_snapshots_airport_iata ON track_snapshots (airport_iata)",
    "CREATE INDEX IF NOT EXISTS ix_track_snapshots_icao24 ON track_snapshots (icao24)",
];
