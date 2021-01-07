// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
const name = Deno.args[0];
// deno-lint-ignore no-explicit-any
const test: { [key: string]: (...args: any[]) => void | Promise<void> } = {
  read(files: string[]): void {
    files.forEach((file) => Deno.readFileSync(file));
  },
  write(files: string[]): void {
    files.forEach((file) =>
      Deno.writeFileSync(file, new Uint8Array(0), { append: true })
    );
  },
  netFetch(urls: string[]): void {
    urls.forEach((url) => fetch(url));
  },
  netListen(endpoints: string[]): void {
    endpoints.forEach((endpoint) => {
      const index = endpoint.lastIndexOf(":");
      const [hostname, port] = [
        endpoint.substr(0, index),
        endpoint.substr(index + 1),
      ];
      const listener = Deno.listen({
        transport: "tcp",
        hostname,
        port: parseInt(port, 10),
      });
      listener.close();
    });
  },
  async netConnect(endpoints: string[]): Promise<void> {
    for (const endpoint of endpoints) {
      const index = endpoint.lastIndexOf(":");
      const [hostname, port] = [
        endpoint.substr(0, index),
        endpoint.substr(index + 1),
      ];
      const listener = await Deno.connect({
        transport: "tcp",
        hostname,
        port: parseInt(port, 10),
      });
      listener.close();
    }
  },
};

if (!test[name]) {
  console.log("Unknown test:", name);
  Deno.exit(1);
}

test[name](Deno.args.slice(1));
