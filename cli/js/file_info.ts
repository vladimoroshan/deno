// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { StatResponse } from "./stat.ts";
import { build } from "./build.ts";

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

  /** The file or directory name. */
  name: string | null;

  /** ID of the device containing the file. Unix only. */
  dev: number | null;

  /** Inode number. Unix only. */
  ino: number | null;

  /** The underlying raw st_mode bits that contain the standard Unix permissions
   * for this file/directory. TODO Match behavior with Go on windows for mode.
   */
  mode: number | null;

  /** Number of hard links pointing to this file. Unix only. */
  nlink: number | null;

  /** User ID of the owner of this file. Unix only. */
  uid: number | null;

  /** User ID of the owner of this file. Unix only. */
  gid: number | null;

  /** Device ID of this file. Unix only. */
  rdev: number | null;

  /** Blocksize for filesystem I/O. Unix only. */
  blksize: number | null;

  /** Number of blocks allocated to the file, in 512-byte units. Unix only. */
  blocks: number | null;

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
  name: string | null;

  dev: number | null;
  ino: number | null;
  mode: number | null;
  nlink: number | null;
  uid: number | null;
  gid: number | null;
  rdev: number | null;
  blksize: number | null;
  blocks: number | null;

  /* @internal */
  constructor(private _res: StatResponse) {
    const isUnix = build.os === "mac" || build.os === "linux";
    const modified = this._res.modified;
    const accessed = this._res.accessed;
    const created = this._res.created;
    const name = this._res.name;
    // Unix only
    const {
      dev,
      ino,
      mode,
      nlink,
      uid,
      gid,
      rdev,
      blksize,
      blocks
    } = this._res;

    this._isFile = this._res.isFile;
    this._isSymlink = this._res.isSymlink;
    this.len = this._res.len;
    this.modified = modified ? modified : null;
    this.accessed = accessed ? accessed : null;
    this.created = created ? created : null;
    this.name = name ? name : null;
    // Only non-null if on Unix
    this.dev = isUnix ? dev : null;
    this.ino = isUnix ? ino : null;
    this.mode = isUnix ? mode : null;
    this.nlink = isUnix ? nlink : null;
    this.uid = isUnix ? uid : null;
    this.gid = isUnix ? gid : null;
    this.rdev = isUnix ? rdev : null;
    this.blksize = isUnix ? blksize : null;
    this.blocks = isUnix ? blocks : null;
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
