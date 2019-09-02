import { Buffer, writeAll } from "./buffer.ts";
import { stdin } from "./files.ts";
import { TextEncoder, TextDecoder } from "./text_encoding.ts";
import { Reader, EOF } from "./io.ts";

export type XevalFunc = (v: string) => void;

// TODO(kevinkassimo): Move this utility to deno_std.
// Import from there once doable.
// Read from reader until EOF and emit string chunks separated
// by the given delimiter.
async function* chunks(
  reader: Reader,
  delim: string
): AsyncIterableIterator<string> {
  const inputBuffer = new Buffer();
  const inspectArr = new Uint8Array(1024);
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  // Avoid unicode problems
  const delimArr = encoder.encode(delim);

  // Record how far we have gone with delimiter matching.
  let nextMatchIndex = 0;
  while (true) {
    let result = await reader.read(inspectArr);
    let rr = result === EOF ? 0 : result;
    if (rr < 0) {
      // Silently fail.
      break;
    }
    const sliceRead = inspectArr.subarray(0, rr);
    // Remember how far we have scanned through inspectArr.
    let nextSliceStartIndex = 0;
    for (let i = 0; i < sliceRead.length; i++) {
      if (sliceRead[i] == delimArr[nextMatchIndex]) {
        // One byte matches with delimiter, move 1 step forward.
        nextMatchIndex++;
      } else {
        // Match delimiter failed. Start from beginning.
        nextMatchIndex = 0;
      }
      // A complete match is found.
      if (nextMatchIndex === delimArr.length) {
        nextMatchIndex = 0; // Reset delim match index.
        const sliceToJoin = sliceRead.subarray(nextSliceStartIndex, i + 1);
        // Record where to start next chunk when a subsequent match is found.
        nextSliceStartIndex = i + 1;
        // Write slice to buffer before processing, since potentially
        // part of the delimiter is stored in the buffer.
        await writeAll(inputBuffer, sliceToJoin);

        let readyBytes = inputBuffer.bytes();
        inputBuffer.reset();
        // Remove delimiter from buffer bytes.
        readyBytes = readyBytes.subarray(
          0,
          readyBytes.length - delimArr.length
        );
        let readyChunk = decoder.decode(readyBytes);
        yield readyChunk;
      }
    }
    // Write all unprocessed chunk to buffer for future inspection.
    await writeAll(inputBuffer, sliceRead.subarray(nextSliceStartIndex));
    if (result === EOF) {
      // Flush the remainder unprocessed chunk.
      const lastChunk = inputBuffer.toString();
      yield lastChunk;
      break;
    }
  }
}

export async function xevalMain(
  xevalFunc: XevalFunc,
  delim_: string | null
): Promise<void> {
  if (!delim_) {
    delim_ = "\n";
  }
  for await (const chunk of chunks(stdin, delim_)) {
    // Ignore empty chunks.
    if (chunk.length > 0) {
      xevalFunc(chunk);
    }
  }
}
