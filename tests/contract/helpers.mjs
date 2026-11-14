// Test helpers: a PNG encoder, a packet reader that follows ErsatzTV's
// ScriptElement.PrepareFromPacket (one frame per call) and the yuva420p frame
// layout. Independent of src/, so the black-box tests run against any build.

import net from "node:net";
import { deflateSync } from "node:zlib";

const MAGIC = 0x47565445;
const Type = { Full: 0, Repeat: 1, Clear: 2, Rectangles: 3 };

/** Bytes of one yuva420p frame: planes Y, U, V, A. */
export function yuvaSize(width, height) {
  return width * height * 2 + Math.ceil(width / 2) * Math.ceil(height / 2) * 2;
}

/** A fully transparent yuva420p frame: Y 16, U/V 128, A 0. */
export function blankYuva(width, height) {
  const frame = Buffer.alloc(yuvaSize(width, height));
  frame.fill(16, 0, width * height);
  frame.fill(128, width * height, frame.length - width * height);
  return frame;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "latin1"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

/** PNG with the given filter on every row (0–4), RGBA or RGB. */
export function encodePng(width, height, rgba, { filter = 0, alpha = true } = {}) {
  const channels = alpha ? 4 : 3;
  const stride = width * channels;
  const pixels = Buffer.alloc(stride * height);
  for (let i = 0, j = 0; i < width * height; i++) {
    for (let c = 0; c < channels; c++) pixels[j++] = rgba[i * 4 + c];
  }

  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = filter;
    for (let x = 0; x < stride; x++) {
      const value = pixels[y * stride + x];
      const left = x >= channels ? pixels[y * stride + x - channels] : 0;
      const up = y > 0 ? pixels[(y - 1) * stride + x] : 0;
      const upLeft = y > 0 && x >= channels ? pixels[(y - 1) * stride + x - channels] : 0;
      const p = left + up - upLeft;
      const pa = Math.abs(p - left);
      const pb = Math.abs(p - up);
      const pc = Math.abs(p - upLeft);
      const predictor = [0, left, up, (left + up) >> 1, pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft][filter];
      raw[y * (stride + 1) + 1 + x] = (value - predictor) & 0xff;
    }
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = alpha ? 6 : 2;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** Frames ErsatzTV would draw from a packet stream: one canvas copy per frame. */
export function readFrames(stream, width, height) {
  const canvas = Buffer.alloc(width * height * 4);
  const frames = [];
  let offset = 0;

  while (offset + 11 <= stream.length) {
    if (stream.readUInt32BE(offset) !== MAGIC || stream.readUInt16BE(offset + 4) !== 1) {
      throw new Error(`bad packet at ${offset}`);
    }
    const type = stream[offset + 6];
    const length = stream.readUInt32BE(offset + 7);
    const payload = stream.subarray(offset + 11, offset + 11 + length);
    if (payload.length < length) break;
    offset += 11 + length;
    let repeat = 0;

    if (type === Type.Full) {
      payload.copy(canvas);
    } else if (type === Type.Clear) {
      canvas.fill(0);
    } else if (type === Type.Repeat) {
      repeat = payload.readUInt32BE(0);
    } else if (type === Type.Rectangles) {
      canvas.fill(0);
      const count = payload.readUInt16BE(0);
      let p = 2;
      for (let i = 0; i < count; i++) {
        const [x, y, w, h] = [0, 2, 4, 6].map((d) => payload.readUInt16BE(p + d));
        p += 8;
        for (let row = 0; row < h; row++) {
          payload.copy(canvas, ((y + row) * width + x) * 4, p + row * w * 4, p + (row + 1) * w * 4);
        }
        p += w * h * 4;
      }
    }

    for (let i = 0; i <= repeat; i++) frames.push(Buffer.from(canvas));
  }

  return frames;
}

/** VizRS stand-in on TCP: `OK {layers}` to STATE, `OK {}` to the rest; `received` = the lines. */
export async function fakeVizrs(layers = []) {
  const received = [];
  const sockets = new Set();
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    socket.setEncoding("utf8");
    let buffer = "";
    socket.on("data", (chunk) => {
      buffer += chunk;
      let index;
      while ((index = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);
        received.push(line);
        socket.write(line.endsWith("STATE") ? `OK ${JSON.stringify({ layers })}\n` : "OK {}\n");
      }
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    received,
    address: `127.0.0.1:${server.address().port}`,
    close: () => {
      for (const socket of sockets) socket.destroy();
      server.close();
    },
  };
}
