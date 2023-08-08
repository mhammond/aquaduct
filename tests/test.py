from aquaduct import *
import asyncio
import unittest
import urllib.parse
from threading import Thread;

# A very messy example of a Python back-end, and Python code going via uniffi to use it.
class TestBackend(unittest.TestCase):
    def test_it(self):
        async def test():
            class PyBackend(Backend):
                def start(self, adaptor):
                    t = Thread(target=request_thread, args=("GET", "http://httpbin.org/bytes/24", adaptor))
                    t.start()

            set_backend(PyBackend())

            reader = new_request_reader()
            while True:
                chunk = await reader.chunk()
                if chunk is None:
                    break
                print("CHUNK", chunk, type(chunk))
            print("DONE")

        asyncio.run(test())

def request_thread(method, url, adaptor):
    try:
        asyncio.run(make_request(method, url, adaptor))
    except Exception as e:
        print("FAILED", e)

async def make_request(method, url, adaptor):
    url = urllib.parse.urlsplit(url)
    if url.scheme == 'https':
        reader, writer = await asyncio.open_connection(
            url.hostname, 443, ssl=True)
    else:
        reader, writer = await asyncio.open_connection(
            url.hostname, 80)

    query = (
        f"{method} {url.path or '/'} HTTP/1.0\r\n"
        f"Host: {url.hostname}\r\n"
        f"\r\n"
    )

    writer.write(query.encode('latin-1'))
    while True:
        line = await reader.readline()
        line = line.decode('latin1').rstrip()
        if not line:
            break
        if line:
            print(f'HTTP header> {line}')

    # Send the body over the aquaduct
    while True:
        line = await reader.read(2048)
        print("ODA", line)
        if not line:
            break
        adaptor.on_data_available(line)
    # Ignore the body, close the socket
    writer.close()
    await writer.wait_closed()
    adaptor.done()
    print("REQ complete")

# An example of Python code using a Rust implemented backend.
class Test(unittest.TestCase):
    def test_raw(self):
        async def test():
            r = Request(Method.GET, "http://httpbin.org/bytes/24")
            resp = await r.send()
            num = 0
            while True:
                chunk = await resp.reader.chunk()
                if chunk is None:
                    break
                num += len(chunk)
            self.assertEqual(num, 24)

        asyncio.run(test())

if __name__ == '__main__':
    unittest.main()
