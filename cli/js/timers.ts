// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { assert } from "./util.ts";
import * as dispatch from "./dispatch.ts";
import { sendSync, sendAsync } from "./dispatch_json.ts";
import { RBTree } from "./rbtree.ts";

const { console } = globalThis;

interface Timer {
  id: number;
  callback: () => void;
  delay: number;
  due: number;
  repeat: boolean;
  scheduled: boolean;
}

// Timeout values > TIMEOUT_MAX are set to 1.
const TIMEOUT_MAX = 2 ** 31 - 1;

let globalTimeoutDue: number | null = null;

let nextTimerId = 1;
const idMap = new Map<number, Timer>();
type DueNode = { due: number; timers: Timer[] };
const dueTree = new RBTree<DueNode>((a, b) => a.due - b.due);

function clearGlobalTimeout(): void {
  globalTimeoutDue = null;
  sendSync(dispatch.OP_GLOBAL_TIMER_STOP);
}

let pendingEvents = 0;
const pendingFireTimers: Timer[] = [];
let hasPendingFireTimers = false;
let pendingScheduleTimers: Timer[] = [];

async function setGlobalTimeout(due: number, now: number): Promise<void> {
  // Since JS and Rust don't use the same clock, pass the time to rust as a
  // relative time value. On the Rust side we'll turn that into an absolute
  // value again.
  const timeout = due - now;
  assert(timeout >= 0);
  // Send message to the backend.
  globalTimeoutDue = due;
  pendingEvents++;
  await sendAsync(dispatch.OP_GLOBAL_TIMER, { timeout });
  pendingEvents--;
  // eslint-disable-next-line @typescript-eslint/no-use-before-define
  fireTimers();
}

function setOrClearGlobalTimeout(due: number | null, now: number): void {
  if (due == null) {
    clearGlobalTimeout();
  } else {
    setGlobalTimeout(due, now);
  }
}

function schedule(timer: Timer, now: number): void {
  assert(!timer.scheduled);
  assert(now <= timer.due);
  // There are more timers pending firing.
  // We must ensure new timer scheduled after them.
  // Push them to a queue that would be depleted after last pending fire
  // timer is fired.
  // (This also implies behavior of setInterval)
  if (hasPendingFireTimers) {
    pendingScheduleTimers.push(timer);
    return;
  }
  // Find or create the list of timers that will fire at point-in-time `due`.
  const maybeNewDueNode = { due: timer.due, timers: [] };
  let dueNode = dueTree.find(maybeNewDueNode);
  if (dueNode === null) {
    dueTree.insert(maybeNewDueNode);
    dueNode = maybeNewDueNode;
  }
  // Append the newly scheduled timer to the list and mark it as scheduled.
  dueNode!.timers.push(timer);
  timer.scheduled = true;
  // If the new timer is scheduled to fire before any timer that existed before,
  // update the global timeout to reflect this.
  if (globalTimeoutDue === null || globalTimeoutDue > timer.due) {
    setOrClearGlobalTimeout(timer.due, now);
  }
}

function unschedule(timer: Timer): void {
  // Check if our timer is pending scheduling or pending firing.
  // If either is true, they are not in tree, and their idMap entry
  // will be deleted soon. Remove it from queue.
  let index = -1;
  if ((index = pendingScheduleTimers.indexOf(timer)) >= 0) {
    pendingScheduleTimers.splice(index);
    return;
  }
  if ((index = pendingFireTimers.indexOf(timer)) >= 0) {
    pendingFireTimers.splice(index);
    return;
  }
  // If timer is not in the 2 pending queues and is unscheduled,
  // it is not in the tree.
  if (!timer.scheduled) {
    return;
  }
  const searchKey = { due: timer.due, timers: [] };
  // Find the list of timers that will fire at point-in-time `due`.
  const list = dueTree.find(searchKey)!.timers;
  if (list.length === 1) {
    // Time timer is the only one in the list. Remove the entire list.
    assert(list[0] === timer);
    dueTree.remove(searchKey);
    // If the unscheduled timer was 'next up', find when the next timer that
    // still exists is due, and update the global alarm accordingly.
    if (timer.due === globalTimeoutDue) {
      const nextDueNode: DueNode | null = dueTree.min();
      setOrClearGlobalTimeout(nextDueNode && nextDueNode.due, Date.now());
    }
  } else {
    // Multiple timers that are due at the same point in time.
    // Remove this timer from the list.
    const index = list.indexOf(timer);
    assert(index > -1);
    list.splice(index, 1);
  }
}

function fire(timer: Timer): void {
  // If the timer isn't found in the ID map, that means it has been cancelled
  // between the timer firing and the promise callback (this function).
  if (!idMap.has(timer.id)) {
    return;
  }
  // Reschedule the timer if it is a repeating one, otherwise drop it.
  if (!timer.repeat) {
    // One-shot timer: remove the timer from this id-to-timer map.
    idMap.delete(timer.id);
  } else {
    // Interval timer: compute when timer was supposed to fire next.
    // However make sure to never schedule the next interval in the past.
    const now = Date.now();
    timer.due = Math.max(now, timer.due + timer.delay);
    schedule(timer, now);
  }
  // Call the user callback. Intermediate assignment is to avoid leaking `this`
  // to it, while also keeping the stack trace neat when it shows up in there.
  const callback = timer.callback;
  callback();
}

function fireTimers(): void {
  const now = Date.now();
  // Bail out if we're not expecting the global timer to fire.
  if (globalTimeoutDue === null || pendingEvents > 0) {
    return;
  }
  // After firing the timers that are due now, this will hold the first timer
  // list that hasn't fired yet.
  let nextDueNode: DueNode | null;
  while ((nextDueNode = dueTree.min()) !== null && nextDueNode.due <= now) {
    dueTree.remove(nextDueNode);
    // Fire all the timers in the list.
    for (const timer of nextDueNode.timers) {
      // With the list dropped, the timer is no longer scheduled.
      timer.scheduled = false;
      // Place the callback to pending timers to fire.
      pendingFireTimers.push(timer);
    }
  }
  if (pendingFireTimers.length > 0) {
    hasPendingFireTimers = true;
    // Fire the list of pending timers as a chain of microtasks.
    globalThis.queueMicrotask(firePendingTimers);
  } else {
    setOrClearGlobalTimeout(nextDueNode && nextDueNode.due, now);
  }
}

function firePendingTimers(): void {
  if (pendingFireTimers.length === 0) {
    // All timer tasks are done.
    hasPendingFireTimers = false;
    // Schedule all new timers pushed during previous timer executions
    const now = Date.now();
    for (const newTimer of pendingScheduleTimers) {
      newTimer.due = Math.max(newTimer.due, now);
      schedule(newTimer, now);
    }
    pendingScheduleTimers = [];
    // Reschedule for next round of timeout.
    const nextDueNode = dueTree.min();
    const due = nextDueNode && Math.max(nextDueNode.due, now);
    setOrClearGlobalTimeout(due, now);
  } else {
    // Fire a single timer and allow its children microtasks scheduled first.
    fire(pendingFireTimers.shift()!);
    // ...and we schedule next timer after this.
    globalThis.queueMicrotask(firePendingTimers);
  }
}

export type Args = unknown[];

function checkThis(thisArg: unknown): void {
  if (thisArg !== null && thisArg !== undefined && thisArg !== globalThis) {
    throw new TypeError("Illegal invocation");
  }
}

function checkBigInt(n: unknown): void {
  if (typeof n === "bigint") {
    throw new TypeError("Cannot convert a BigInt value to a number");
  }
}

function setTimer(
  cb: (...args: Args) => void,
  delay: number,
  args: Args,
  repeat: boolean
): number {
  // Bind `args` to the callback and bind `this` to globalThis(global).
  const callback: () => void = cb.bind(globalThis, ...args);
  // In the browser, the delay value must be coercible to an integer between 0
  // and INT32_MAX. Any other value will cause the timer to fire immediately.
  // We emulate this behavior.
  const now = Date.now();
  if (delay > TIMEOUT_MAX) {
    console.warn(
      `${delay} does not fit into` +
        " a 32-bit signed integer." +
        "\nTimeout duration was set to 1."
    );
    delay = 1;
  }
  delay = Math.max(0, delay | 0);

  // Create a new, unscheduled timer object.
  const timer = {
    id: nextTimerId++,
    callback,
    args,
    delay,
    due: now + delay,
    repeat,
    scheduled: false
  };
  // Register the timer's existence in the id-to-timer map.
  idMap.set(timer.id, timer);
  // Schedule the timer in the due table.
  schedule(timer, now);
  return timer.id;
}

/** Sets a timer which executes a function once after the timer expires. */
export function setTimeout(
  cb: (...args: Args) => void,
  delay = 0,
  ...args: Args
): number {
  checkBigInt(delay);
  // @ts-ignore
  checkThis(this);
  return setTimer(cb, delay, args, false);
}

/** Repeatedly calls a function, with a fixed time delay between each call. */
export function setInterval(
  cb: (...args: Args) => void,
  delay = 0,
  ...args: Args
): number {
  checkBigInt(delay);
  // @ts-ignore
  checkThis(this);
  return setTimer(cb, delay, args, true);
}

/** Clears a previously set timer by id. AKA clearTimeout and clearInterval. */
function clearTimer(id: number): void {
  id = Number(id);
  const timer = idMap.get(id);
  if (timer === undefined) {
    // Timer doesn't exist any more or never existed. This is not an error.
    return;
  }
  // Unschedule the timer if it is currently scheduled, and forget about it.
  unschedule(timer);
  idMap.delete(timer.id);
}

export function clearTimeout(id = 0): void {
  checkBigInt(id);
  if (id === 0) {
    return;
  }
  clearTimer(id);
}

export function clearInterval(id = 0): void {
  checkBigInt(id);
  if (id === 0) {
    return;
  }
  clearTimer(id);
}
