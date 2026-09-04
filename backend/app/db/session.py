from __future__ import annotations

from collections.abc import AsyncIterator

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine
from sqlalchemy.pool import NullPool

from app.config import get_settings
from app.db.tables import Base

_engine = None
_factory = None


def _make() -> tuple:
    settings = get_settings()
    engine = create_async_engine(
        settings.database_url,
        poolclass=NullPool,
        echo=False,
    )
    factory = async_sessionmaker(engine, expire_on_commit=False, class_=AsyncSession)
    return engine, factory


def engine():
    global _engine, _factory
    if _engine is None:
        _engine, _factory = _make()
    return _engine


def session_factory() -> async_sessionmaker[AsyncSession]:
    global _engine, _factory
    if _factory is None:
        _engine, _factory = _make()
    return _factory


_ALTERS = (
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS continent VARCHAR(8) DEFAULT ''",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS country_name TEXT DEFAULT ''",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS region_name TEXT DEFAULT ''",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS home_link TEXT",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS gps_code VARCHAR(16)",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS local_code VARCHAR(16)",
    "ALTER TABLE airports ADD COLUMN IF NOT EXISTS keywords TEXT",
)


async def init_db() -> None:
    async with engine().begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
        from sqlalchemy import text

        for stmt in _ALTERS:
            await conn.execute(text(stmt))
        for ext in ("CREATE EXTENSION IF NOT EXISTS unaccent", "CREATE EXTENSION IF NOT EXISTS pg_trgm"):
            try:
                await conn.execute(text(ext))
            except Exception:
                pass


async def get_session() -> AsyncIterator[AsyncSession]:
    async with session_factory()() as session:
        yield session
