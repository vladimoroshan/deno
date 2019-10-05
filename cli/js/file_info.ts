// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { StatResponse } from "./stat.ts";

/** A FileInfo describes a file and is returned by `stat`, `lstat`,
 * `statSync`, `lstatSync`.
 */
export interface FileInfo {
  /** The size of the file, in bytes. */
  len: number;
  /** The last modification time of the file. This corresponds to the `mtime`
   * field from `stat` on Unix and `ftLastWriteTime` on Windows. This may not
   * be available on all platforms.
   */
  modified: number | null;
  /** The last access time of the file. This corresponds to the `atime`
   * field from `stat` on Unix and `ftLastAccessTime` on Windows. This may not
   * be available on all platforms.
   */
  accessed: number | null;
  /** The last access time of the file. This corresponds to the `birthtime`
   * field from `stat` on Unix and `ftCreationTime` on Windows. This may not
   * be available on all platforms.
   */
  created: number | null;
  /** The underlying raw st_mode bits that contain the standard Unix permissions
   * for this file/directory. TODO Match behavior with Go on windows for mode.
   */
  mode: number | null;

  /** The file or directory name. */
  name: string | null;

  /** Returns whether this is info for a regular file. This result is mutually
   * exclusive to `FileInfo.isDirectory` and `FileInfo.isSymlink`.
   */
  isFile(): boolean;

  /** Returns whether this is info for a regular directory. This result is
   * mutually exclusive to `FileInfo.isFile` and `FileInfo.isSymlink`.
   */
  isDirectory(): boolean;

  /** Returns whether this is info for a symlink. This result is
   * mutually exclusive to `FileInfo.isFile` and `FileInfo.isDirectory`.
   */
  isSymlink(): boolean;
}

// @internal
export class FileInfoImpl implements FileInfo {
  private readonly _isFile: boolean;
  private readonly _isSymlink: boolean;
  len: number;
  modified: number | null;
  accessed: number | null;
  created: number | null;
  mode: number | null;
  name: string | null;

  /* @internal */
  constructor(private _res: StatResponse) {
    const modified = this._res.modified;
    const accessed = this._res.accessed;
    const created = this._res.created;
    const hasMode = this._res.hasMode;
    const mode = this._res.mode; // negative for invalid mode (Windows)
    const name = this._res.name;

    this._isFile = this._res.isFile;
    this._isSymlink = this._res.isSymlink;
    this.len = this._res.len;
    this.modified = modified ? modified : null;
    this.accessed = accessed ? accessed : null;
    this.created = created ? created : null;
    // null on Windows
    this.mode = hasMode ? mode : null;
    this.name = name ? name : null;
  }

  isFile(): boolean {
    return this._isFile;
  }

  isDirectory(): boolean {
    return !this._isFile && !this._isSymlink;
  }

  isSymlink(): boolean {
    return this._isSymlink;
  }
}
