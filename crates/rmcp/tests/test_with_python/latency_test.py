#!/usr/bin/env python3
"""Simple latency test - matches Rust version exactly."""
import sys
import time
import asyncio
from mcp import ClientSession
from mcp.client.streamable_http import streamable_http_client


async def main():
    url, tool = sys.argv[1], sys.argv[2]

    async with streamable_http_client(url=url) as (read, write, _):
        async with ClientSession(read, write) as session:
            await session.initialize()

            for i in range(1, 6):
                start = time.perf_counter()
                if tool == "ping":
                    await session.send_ping()
                else:
                    await session.call_tool(tool, arguments={})
                elapsed = (time.perf_counter() - start) * 1000

                print(f"Call {i}: {elapsed:.1f}ms")


if __name__ == "__main__":
    asyncio.run(main())