import asyncio
import os
import sys

from scheduler.database import deactivate_expired_tasks
from scheduler.env import load_enviroment
from scheduler.logger import logger


async def main() -> None:
    load_enviroment()

    if not os.environ["debug"]:
        print("Failed to startup scheduler: .env was not loaded well")
        sys.exit(1)


    logger.info("Enviroment loaded successfully")

    while True:
        await deactivate_expired_tasks()


if __name__ == "__main__":
    asyncio.run(main())
