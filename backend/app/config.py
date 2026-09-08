from functools import lru_cache

from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    database_url: str = "postgresql+asyncpg://matthieutohme@127.0.0.1:5432/skiplagging"
    amadeus_client_id: str = ""
    amadeus_client_secret: str = ""
    amadeus_hostname: str = "test"
    duffel_token: str = ""
    rapidapi_key: str = ""
    opensky_client_id: str = ""
    opensky_client_secret: str = ""
    cache_ttl_seconds: int = 180
    offer_ttl_seconds: int = 900
    max_hidden_candidates: int = 8
    fast_candidates: int = 3
    candidate_min_score: float = 0.35
    search_cost_usd: float = 0.005
    max_concurrency: int = 4
    mock_enabled: bool = True
    min_hidden_saving: float = 20.0
    max_provider_calls: int = 12
    cors_origins: str = "http://localhost:5173,http://127.0.0.1:5173"
    # Duffel sandbox hangs on some pairs (STN→LHR). Comma-separated IATA.
    discover_skip_dests: str = "STN"

    model_config = SettingsConfigDict(
        env_file=("../.env", ".env"),
        env_file_encoding="utf-8",
        extra="ignore",
    )

    @property
    def origin_list(self) -> list[str]:
        return [o.strip() for o in self.cors_origins.split(",") if o.strip()]

    @property
    def skip_dests(self) -> set[str]:
        return {c.strip().upper() for c in self.discover_skip_dests.split(",") if c.strip()}

    @property
    def amadeus_enabled(self) -> bool:
        return bool(self.amadeus_client_id and self.amadeus_client_secret)

    @property
    def duffel_enabled(self) -> bool:
        return bool(self.duffel_token)

    @property
    def aerodatabox_enabled(self) -> bool:
        return bool(self.rapidapi_key)

    @property
    def amadeus_base(self) -> str:
        host = "api.amadeus.com" if self.amadeus_hostname == "production" else "test.api.amadeus.com"
        return f"https://{host}"


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    return Settings()
