import os
from asyncio import sleep
from datetime import UTC, datetime

import asyncpg

from .logger import logger


async def deactivate_expired_tasks():
    now = datetime.now(UTC)

    conn = await asyncpg.connect(dsn=os.environ["database_url"])
    try:
        status = await conn.execute(
            """
            UPDATE tasks
            SET is_active = false
            WHERE is_active = true AND ends_at < $1
            """,
            now
        )

        touched = int(status.split()[1])

        if touched > 0:
            logger.info(f"[{now}] Updated: {touched}")
            await sleep(10)

    finally:
        await conn.close()
