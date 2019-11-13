import {
  GlobOptions,
  SEP_PATTERN,
  globToRegExp,
  isAbsolute,
  isGlob,
  isWindows,
  joinGlobs,
  normalize
} from "../path/mod.ts";
import { WalkInfo, walk, walkSync } from "./walk.ts";
const { cwd, stat, statSync } = Deno;
type FileInfo = Deno.FileInfo;

export interface ExpandGlobOptions extends GlobOptions {
  root?: string;
  exclude?: string[];
  includeDirs?: boolean;
}

interface SplitPath {
  segments: string[];
  isAbsolute: boolean;
  hasTrailingSep: boolean;
  // Defined for any absolute Windows path.
  winRoot?: string;
}

// TODO: Maybe make this public somewhere.
function split(path: string): SplitPath {
  const s = SEP_PATTERN.source;
  const segments = path
    .replace(new RegExp(`^${s}|${s}$`, "g"), "")
    .split(SEP_PATTERN);
  const isAbsolute_ = isAbsolute(path);
  return {
    segments,
    isAbsolute: isAbsolute_,
    hasTrailingSep: !!path.match(new RegExp(`${s}$`)),
    winRoot: isWindows && isAbsolute_ ? segments.shift() : undefined
  };
}

/**
 * Expand the glob string from the specified `root` directory and yield each
 * result as a `WalkInfo` object.
 */
// TODO: Use a proper glob expansion algorithm.
// This is a very incomplete solution. The whole directory tree from `root` is
// walked and parent paths are not supported.
export async function* expandGlob(
  glob: string,
  {
    root = cwd(),
    exclude = [],
    includeDirs = true,
    extended = false,
    globstar = false
  }: ExpandGlobOptions = {}
): AsyncIterableIterator<WalkInfo> {
  const globOptions: GlobOptions = { extended, globstar };
  const absRoot = isAbsolute(root)
    ? normalize(root)
    : joinGlobs([cwd(), root], globOptions);
  const resolveFromRoot = (path: string): string =>
    isAbsolute(path)
      ? normalize(path)
      : joinGlobs([absRoot, path], globOptions);
  const excludePatterns = exclude
    .map(resolveFromRoot)
    .map((s: string): RegExp => globToRegExp(s, globOptions));
  const shouldInclude = ({ filename }: WalkInfo): boolean =>
    !excludePatterns.some((p: RegExp): boolean => !!filename.match(p));
  const { segments, hasTrailingSep, winRoot } = split(resolveFromRoot(glob));

  let fixedRoot = winRoot != undefined ? winRoot : "/";
  while (segments.length > 0 && !isGlob(segments[0])) {
    fixedRoot = joinGlobs([fixedRoot, segments.shift()!], globOptions);
  }

  let fixedRootInfo: WalkInfo;
  try {
    fixedRootInfo = { filename: fixedRoot, info: await stat(fixedRoot) };
  } catch {
    return;
  }

  async function* advanceMatch(
    walkInfo: WalkInfo,
    globSegment: string
  ): AsyncIterableIterator<WalkInfo> {
    if (!walkInfo.info.isDirectory()) {
      return;
    } else if (globSegment == "..") {
      const parentPath = joinGlobs([walkInfo.filename, ".."], globOptions);
      try {
        return yield* [
          { filename: parentPath, info: await stat(parentPath) }
        ].filter(shouldInclude);
      } catch {
        return;
      }
    } else if (globSegment == "**") {
      return yield* walk(walkInfo.filename, {
        includeFiles: false,
        skip: excludePatterns
      });
    }
    yield* walk(walkInfo.filename, {
      maxDepth: 1,
      match: [
        globToRegExp(
          joinGlobs([walkInfo.filename, globSegment], globOptions),
          globOptions
        )
      ],
      skip: excludePatterns
    });
  }

  let currentMatches: WalkInfo[] = [fixedRootInfo];
  for (const segment of segments) {
    // Advancing the list of current matches may introduce duplicates, so we
    // pass everything through this Map.
    const nextMatchMap: Map<string, FileInfo> = new Map();
    for (const currentMatch of currentMatches) {
      for await (const nextMatch of advanceMatch(currentMatch, segment)) {
        nextMatchMap.set(nextMatch.filename, nextMatch.info);
      }
    }
    currentMatches = [...nextMatchMap].sort().map(
      ([filename, info]): WalkInfo => ({
        filename,
        info
      })
    );
  }
  if (hasTrailingSep) {
    currentMatches = currentMatches.filter(({ info }): boolean =>
      info.isDirectory()
    );
  }
  if (!includeDirs) {
    currentMatches = currentMatches.filter(
      ({ info }): boolean => !info.isDirectory()
    );
  }
  yield* currentMatches;
}

/** Synchronous version of `expandGlob()`. */
// TODO: As `expandGlob()`.
export function* expandGlobSync(
  glob: string,
  {
    root = cwd(),
    exclude = [],
    includeDirs = true,
    extended = false,
    globstar = false
  }: ExpandGlobOptions = {}
): IterableIterator<WalkInfo> {
  const globOptions: GlobOptions = { extended, globstar };
  const absRoot = isAbsolute(root)
    ? normalize(root)
    : joinGlobs([cwd(), root], globOptions);
  const resolveFromRoot = (path: string): string =>
    isAbsolute(path)
      ? normalize(path)
      : joinGlobs([absRoot, path], globOptions);
  const excludePatterns = exclude
    .map(resolveFromRoot)
    .map((s: string): RegExp => globToRegExp(s, globOptions));
  const shouldInclude = ({ filename }: WalkInfo): boolean =>
    !excludePatterns.some((p: RegExp): boolean => !!filename.match(p));
  const { segments, hasTrailingSep, winRoot } = split(resolveFromRoot(glob));

  let fixedRoot = winRoot != undefined ? winRoot : "/";
  while (segments.length > 0 && !isGlob(segments[0])) {
    fixedRoot = joinGlobs([fixedRoot, segments.shift()!], globOptions);
  }

  let fixedRootInfo: WalkInfo;
  try {
    fixedRootInfo = { filename: fixedRoot, info: statSync(fixedRoot) };
  } catch {
    return;
  }

  function* advanceMatch(
    walkInfo: WalkInfo,
    globSegment: string
  ): IterableIterator<WalkInfo> {
    if (!walkInfo.info.isDirectory()) {
      return;
    } else if (globSegment == "..") {
      const parentPath = joinGlobs([walkInfo.filename, ".."], globOptions);
      try {
        return yield* [
          { filename: parentPath, info: statSync(parentPath) }
        ].filter(shouldInclude);
      } catch {
        return;
      }
    } else if (globSegment == "**") {
      return yield* walkSync(walkInfo.filename, {
        includeFiles: false,
        skip: excludePatterns
      });
    }
    yield* walkSync(walkInfo.filename, {
      maxDepth: 1,
      match: [
        globToRegExp(
          joinGlobs([walkInfo.filename, globSegment], globOptions),
          globOptions
        )
      ],
      skip: excludePatterns
    });
  }

  let currentMatches: WalkInfo[] = [fixedRootInfo];
  for (const segment of segments) {
    // Advancing the list of current matches may introduce duplicates, so we
    // pass everything through this Map.
    const nextMatchMap: Map<string, FileInfo> = new Map();
    for (const currentMatch of currentMatches) {
      for (const nextMatch of advanceMatch(currentMatch, segment)) {
        nextMatchMap.set(nextMatch.filename, nextMatch.info);
      }
    }
    currentMatches = [...nextMatchMap].sort().map(
      ([filename, info]): WalkInfo => ({
        filename,
        info
      })
    );
  }
  if (hasTrailingSep) {
    currentMatches = currentMatches.filter(({ info }): boolean =>
      info.isDirectory()
    );
  }
  if (!includeDirs) {
    currentMatches = currentMatches.filter(
      ({ info }): boolean => !info.isDirectory()
    );
  }
  yield* currentMatches;
}
