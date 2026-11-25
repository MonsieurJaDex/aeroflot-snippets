// cellAfterAck: снап, булавка, отказ и запись legacy на 404.
import assert from "node:assert/strict";
import test from "node:test";
import { cellAfterAck } from "./gpsSync.js";

test("снап: серверная клетка побеждает локальную", () => {
  const result = cellAfterAck({
    applied: true,
    httpStatus: 200,
    serverCell: [12, 34],
    localCell: [1, 2],
    previousCell: [5, 6],
  });
  assert.deepEqual(result, { cell: [12, 34], writeLegacy: false });
});

test("булавка: applied и чужая serverCell, не localCell", () => {
  const pin = [80, 90];
  const local = [3, 4];
  const result = cellAfterAck({
    applied: true,
    httpStatus: 200,
    serverCell: pin,
    localCell: local,
    previousCell: [7, 8],
  });
  assert.deepEqual(result, { cell: pin, writeLegacy: false });
  assert.notDeepEqual(result.cell, local);
});

test("отказ: applied false сохраняет previous, writeLegacy false", () => {
  const result = cellAfterAck({
    applied: false,
    httpStatus: 200,
    serverCell: [12, 34],
    localCell: [1, 2],
    previousCell: [5, 6],
  });
  assert.deepEqual(result, { cell: [5, 6], writeLegacy: false });
});

test("404 пишет legacy локальной клеткой", () => {
  const result = cellAfterAck({
    applied: false,
    httpStatus: 404,
    serverCell: null,
    localCell: [9, 10],
    previousCell: [5, 6],
  });
  assert.deepEqual(result, { cell: [9, 10], writeLegacy: true });
});

test("applied без serverCell берёт localCell и не пишет legacy", () => {
  const result = cellAfterAck({
    applied: true,
    httpStatus: 200,
    serverCell: [Number.NaN, 1],
    localCell: [4, 5],
    previousCell: [6, 7],
  });
  assert.deepEqual(result, { cell: [4, 5], writeLegacy: false });
});

test("без applied сохраняет previous", () => {
  const result = cellAfterAck({
    httpStatus: 200,
    serverCell: [1, 1],
    localCell: [2, 2],
    previousCell: [3, 3],
  });
  assert.deepEqual(result, { cell: [3, 3], writeLegacy: false });
});
