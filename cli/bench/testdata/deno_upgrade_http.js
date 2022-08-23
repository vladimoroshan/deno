const { serve, upgradeHttp } = Deno;
const u8 = Deno.core.encode("HTTP/1.1 101 Switching Protocols\r\n\r\n");

async function handler(req) {
  const [conn, _firstPacket] = upgradeHttp(req);
  await conn.write(u8);
  await conn.close();
}

serve(handler, { hostname: "127.0.0.1", port: 9000 });
