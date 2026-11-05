-- Misty Engine test framework, loaded before every test file (see docs/scripting.md, "Testing").
-- describe/it/hooks collect a tree of tests; the Test Runner then calls __misty.run(i, next)
-- inside a coroutine for each test it runs.

local M = { tests = {}, mode = "edit", ignoreErrors = false, timeout = nil }
__misty = M

-- Game scripts have no `debug` library; tests don't either (the framework keeps what it needs).
local getinfo, traceback = debug.getinfo, debug.traceback
debug = nil

local function new_suite(name, parent, skip, only)
    return {
        name = name,
        parent = parent,
        skip = skip,
        only = only,
        beforeAll = {},
        afterAll = {},
        beforeEach = {},
        afterEach = {},
    }
end

local root = new_suite(nil, nil, false, false)
local current = root

-- ---- Declaring tests ---------------------------------------------------------------------------

local function run_describe(name, fn, skip, only)
    if type(name) ~= "string" or type(fn) ~= "function" then
        error("describe(name, fn) needs a name and a function", 3)
    end
    local s = new_suite(name, current, current.skip or skip, only)
    local prev = current
    current = s
    local ok, err = pcall(fn)
    current = prev
    if not ok then
        error(err, 0)
    end
end

describe = setmetatable({
    skip = function(name, fn) run_describe(name, fn, true, false) end,
    only = function(name, fn) run_describe(name, fn, false, true) end,
}, { __call = function(_, name, fn) run_describe(name, fn, false, false) end })
xdescribe = describe.skip

local function add_test(name, fn, skip, only)
    -- Level 1 is add_test, 2 the it/it.skip wrapper, 3 the test file.
    if type(name) ~= "string" then
        error("it(name, fn): the name must be a string", 3)
    end
    if fn ~= nil and type(fn) ~= "function" then
        error("it(name, fn): fn must be a function", 3)
    end
    local info = getinfo(3, "l")
    M.tests[#M.tests + 1] = {
        name = name,
        fn = fn,
        suite = current,
        line = info and info.currentline or 0,
        -- A test without a function is pending (listed as skipped).
        skip = skip or current.skip or fn == nil,
        only = only,
    }
end

it = setmetatable({
    skip = function(name, fn) add_test(name, fn, true, false) end,
    only = function(name, fn) add_test(name, fn, false, true) end,
}, { __call = function(_, name, fn) add_test(name, fn, false, false) end })
test = it
xit = it.skip

local function hook(kind)
    return function(fn)
        if type(fn) ~= "function" then
            error(kind .. "(fn) needs a function", 2)
        end
        local list = current[kind]
        list[#list + 1] = fn
    end
end
beforeAll = hook("beforeAll")
afterAll = hook("afterAll")
beforeEach = hook("beforeEach")
afterEach = hook("afterEach")

-- ---- Values in messages -----------------------------------------------------------------------

local function fmt(v, depth)
    depth = depth or 0
    local tv = type(v)
    if tv == "string" then
        return string.format("%q", v)
    elseif tv == "table" then
        local mt = getmetatable(v)
        if mt and mt.__tostring then
            return tostring(v)
        end
        if depth > 1 then
            return "{...}"
        end
        local parts, n = {}, 0
        for k, x in pairs(v) do
            n = n + 1
            if n > 8 then
                parts[#parts + 1] = "..."
                break
            end
            if math.type(k) == "integer" then
                parts[#parts + 1] = fmt(x, depth + 1)
            else
                parts[#parts + 1] = tostring(k) .. " = " .. fmt(x, depth + 1)
            end
        end
        return "{" .. table.concat(parts, ", ") .. "}"
    end
    return tostring(v)
end
M.fmt = fmt

local function errmsg(e)
    if type(e) == "string" then
        return e
    elseif type(e) == "table" and e.message ~= nil then
        return tostring(e.message)
    end
    return tostring(e)
end
M.errmsg = errmsg

local function deep_equal(a, b, depth)
    if a == b then
        return true
    end
    if type(a) ~= "table" or type(b) ~= "table" or (depth or 0) > 32 then
        return false
    end
    for k, v in pairs(a) do
        if not deep_equal(v, b[k], (depth or 0) + 1) then
            return false
        end
    end
    for k in pairs(b) do
        if a[k] == nil then
            return false
        end
    end
    return true
end

-- Numbers, or the x/y/z/w of vectors.
local function components(v)
    if type(v) == "number" then
        return { v }
    end
    if type(v) ~= "userdata" and type(v) ~= "table" then
        return nil
    end
    local out = {}
    for _, k in ipairs({ "x", "y", "z", "w" }) do
        local ok, c = pcall(function() return v[k] end)
        if ok and type(c) == "number" then
            out[#out + 1] = c
        elseif k == "x" or k == "y" then
            return nil
        else
            break
        end
    end
    return out
end

local function near(a, b, eps)
    local ca, cb = components(a), components(b)
    if ca == nil or cb == nil or #ca ~= #cb then
        return false
    end
    for i = 1, #ca do
        if math.abs(ca[i] - cb[i]) > eps then
            return false
        end
    end
    return true
end

local function contains(haystack, needle)
    if type(haystack) == "string" then
        return string.find(haystack, tostring(needle), 1, true) ~= nil
    elseif type(haystack) == "table" then
        for _, v in pairs(haystack) do
            if v == needle or deep_equal(v, needle) then
                return true
            end
        end
    end
    return false
end

local function throws(fn, pattern)
    local ok, err = pcall(fn)
    if ok then
        return false, "expected the function to throw an error"
    end
    local m = errmsg(err)
    if pattern ~= nil and not string.find(m, pattern, 1, true) then
        return false, "expected an error containing " .. fmt(pattern) .. ", got " .. fmt(m)
    end
    return true, m
end

local function prefix(msg)
    if msg ~= nil then
        return tostring(msg) .. ": "
    end
    return ""
end

-- ---- assert -----------------------------------------------------------------------------------
-- Every check reports at the line of the test that called it (error level 2).

local A = {}

function A.equal(actual, expected, msg)
    if not deep_equal(actual, expected) then
        error(prefix(msg) .. "expected " .. fmt(expected) .. ", got " .. fmt(actual), 2)
    end
end

function A.notEqual(actual, unexpected, msg)
    if deep_equal(actual, unexpected) then
        error(prefix(msg) .. "expected a value other than " .. fmt(unexpected), 2)
    end
end

function A.near(actual, expected, eps, msg)
    eps = eps or 1e-6
    if not near(actual, expected, eps) then
        error(prefix(msg) .. "expected " .. fmt(expected) .. " ± " .. eps .. ", got " .. fmt(actual), 2)
    end
end

function A.isTrue(v, msg)
    if v ~= true then
        error(prefix(msg) .. "expected true, got " .. fmt(v), 2)
    end
end

function A.isFalse(v, msg)
    if v ~= false then
        error(prefix(msg) .. "expected false, got " .. fmt(v), 2)
    end
end

function A.isNil(v, msg)
    if v ~= nil then
        error(prefix(msg) .. "expected nil, got " .. fmt(v), 2)
    end
end

function A.notNil(v, msg)
    if v == nil then
        error(prefix(msg) .. "expected a value, got nil", 2)
    end
end

function A.greater(a, b, msg)
    if not (a > b) then
        error(prefix(msg) .. "expected " .. fmt(a) .. " to be greater than " .. fmt(b), 2)
    end
end

function A.less(a, b, msg)
    if not (a < b) then
        error(prefix(msg) .. "expected " .. fmt(a) .. " to be less than " .. fmt(b), 2)
    end
end

function A.contains(haystack, needle, msg)
    if not contains(haystack, needle) then
        error(prefix(msg) .. "expected " .. fmt(haystack) .. " to contain " .. fmt(needle), 2)
    end
end

function A.throws(fn, pattern, msg)
    local ok, m = throws(fn, pattern)
    if not ok then
        error(prefix(msg) .. m, 2)
    end
    return m
end

function A.fail(msg)
    error(msg or "failed", 2)
end

-- `assert(v, msg)` keeps working as Lua's own assert.
assert = setmetatable(A, {
    __call = function(_, v, msg, ...)
        if not v then
            error(msg or "assertion failed!", 2)
        end
        return v, msg, ...
    end,
})

-- ---- expect(x).toBe(y) ------------------------------------------------------------------------

function expect(actual)
    local e, never = {}, {}
    -- Both `expect(x).toBe(y)` and `expect(x):toBe(y)`; the tail call keeps the error level.
    local function method(t, f)
        return function(a, ...)
            if a == t then
                return f(...)
            end
            return f(a, ...)
        end
    end
    e.toBe = method(e, function(expected, msg)
        if actual ~= expected then
            error(prefix(msg) .. "expected " .. fmt(expected) .. ", got " .. fmt(actual), 2)
        end
    end)
    e.toEqual = method(e, function(expected, msg)
        if not deep_equal(actual, expected) then
            error(prefix(msg) .. "expected " .. fmt(expected) .. ", got " .. fmt(actual), 2)
        end
    end)
    e.toBeNear = method(e, function(expected, eps, msg)
        eps = eps or 1e-6
        if not near(actual, expected, eps) then
            error(prefix(msg) .. "expected " .. fmt(expected) .. " ± " .. eps .. ", got " .. fmt(actual), 2)
        end
    end)
    e.toBeTruthy = method(e, function(msg)
        if not actual then
            error(prefix(msg) .. "expected a truthy value, got " .. fmt(actual), 2)
        end
    end)
    e.toBeFalsy = method(e, function(msg)
        if actual then
            error(prefix(msg) .. "expected a falsy value, got " .. fmt(actual), 2)
        end
    end)
    e.toBeNil = method(e, function(msg)
        if actual ~= nil then
            error(prefix(msg) .. "expected nil, got " .. fmt(actual), 2)
        end
    end)
    e.toBeGreaterThan = method(e, function(n, msg)
        if not (actual > n) then
            error(prefix(msg) .. "expected " .. fmt(actual) .. " to be greater than " .. fmt(n), 2)
        end
    end)
    e.toBeLessThan = method(e, function(n, msg)
        if not (actual < n) then
            error(prefix(msg) .. "expected " .. fmt(actual) .. " to be less than " .. fmt(n), 2)
        end
    end)
    e.toContain = method(e, function(x, msg)
        if not contains(actual, x) then
            error(prefix(msg) .. "expected " .. fmt(actual) .. " to contain " .. fmt(x), 2)
        end
    end)
    e.toThrow = method(e, function(pattern, msg)
        local ok, m = throws(actual, pattern)
        if not ok then
            error(prefix(msg) .. m, 2)
        end
    end)
    never.toBe = method(never, function(x, msg)
        if actual == x then
            error(prefix(msg) .. "expected a value other than " .. fmt(x), 2)
        end
    end)
    never.toEqual = method(never, function(x, msg)
        if deep_equal(actual, x) then
            error(prefix(msg) .. "expected a value other than " .. fmt(x), 2)
        end
    end)
    never.toBeNil = method(never, function(msg)
        if actual == nil then
            error(prefix(msg) .. "expected a value, got nil", 2)
        end
    end)
    never.toContain = method(never, function(x, msg)
        if contains(actual, x) then
            error(prefix(msg) .. "expected " .. fmt(actual) .. " not to contain " .. fmt(x), 2)
        end
    end)
    e.never = never
    return e
end

-- ---- PlayMode: waiting for frames, loading scenes ---------------------------------------------

local function need_play(what)
    if M.mode ~= "play" then
        error(what .. " only works in PlayMode tests (*.playtest.lua)", 3)
    end
end

-- Wait `n` game frames (1 by default).
function yieldFrames(n)
    need_play("yieldFrames")
    n = math.max(1, math.floor(n or 1))
    coroutine.yield(n)
end

-- Wait `seconds` of game time (at least one frame).
function yield(seconds)
    need_play("yield")
    local target = Time.time + (seconds or 0)
    repeat
        coroutine.yield(1)
    until Time.time >= target - 1e-6
end

-- Wait until `cond()` is true; fails after `timeout` seconds of game time (10 by default).
function waitUntil(cond, timeout)
    need_play("waitUntil")
    if type(cond) ~= "function" then
        error("waitUntil(fn, timeout) needs a function", 2)
    end
    timeout = timeout or 10
    local start = Time.time
    while not cond() do
        if Time.time - start >= timeout then
            error("waitUntil: the condition was not met within " .. timeout .. " s", 2)
        end
        coroutine.yield(1)
    end
end

-- Replace the scene: in PlayMode the play session restarts on it (its Awake/Start run on the
-- next frame); in EditMode it becomes the test's scratch scene.
function loadScene(name)
    if type(name) ~= "string" then
        error("loadScene(name) needs a scene name or path", 2)
    end
    M.loadError = nil
    coroutine.yield({ loadScene = name })
    if M.loadError ~= nil then
        local e = M.loadError
        M.loadError = nil
        error(e, 2)
    end
end

-- Errors logged while a PlayMode test runs fail it, unless the test calls this.
function ignoreErrorLogs()
    M.ignoreErrors = true
end

-- Longest game time this PlayMode test may run (30 s by default).
function setTimeout(seconds)
    M.timeout = seconds
end

-- ---- Listing and running (called by the Test Runner) ------------------------------------------

local function suite_only(s)
    while s do
        if s.only then
            return true
        end
        s = s.parent
    end
    return false
end

function M.list()
    local any_only = false
    for _, t in ipairs(M.tests) do
        if t.only or suite_only(t.suite) then
            any_only = true
        end
    end
    local out = {}
    for i, t in ipairs(M.tests) do
        local path = {}
        local s = t.suite
        while s and s.name do
            table.insert(path, 1, s.name)
            s = s.parent
        end
        local selected = t.only or suite_only(t.suite)
        out[i] = { name = t.name, path = path, line = t.line, skip = t.skip or (any_only and not selected) }
    end
    return out
end

local function chain(suite)
    local c = {}
    while suite do
        table.insert(c, 1, suite)
        suite = suite.parent
    end
    return c
end

local function call(fn)
    local tb
    local ok, err = xpcall(fn, function(e)
        tb = traceback("", 2)
        return e
    end)
    return ok, err, tb
end

local entered = {}

-- afterAll of suites the next test is not in (innermost first).
local function leave(c, nextIndex, fail)
    local keep = {}
    if nextIndex ~= nil and M.tests[nextIndex] ~= nil then
        for _, s in ipairs(chain(M.tests[nextIndex].suite)) do
            keep[s] = true
        end
    end
    for i = #c, 1, -1 do
        local s = c[i]
        if entered[s] and not keep[s] then
            entered[s] = nil
            for _, h in ipairs(s.afterAll) do
                local ok, e, tb = call(h)
                if not ok then
                    fail("afterAll", e, tb)
                end
            end
        end
    end
end

-- Run test `index` with its hooks; `nextIndex` is the next test this run will execute (its
-- suites keep their beforeAll state). Returns { status, message, traceback }.
function M.run(index, nextIndex)
    local t = M.tests[index]
    M.ignoreErrors = false
    M.timeout = nil
    local result = { status = "passed" }
    local function fail(phase, e, tb)
        if result.status == "passed" then
            result.status = "failed"
            result.message = (phase and (phase .. " failed: ") or "") .. errmsg(e)
            result.traceback = tb
        end
    end
    local c = chain(t.suite)
    for _, s in ipairs(c) do
        if not entered[s] then
            entered[s] = true
            for _, h in ipairs(s.beforeAll) do
                local ok, e, tb = call(h)
                if not ok then
                    s.failed = { e, tb }
                    break
                end
            end
        end
        if s.failed then
            fail("beforeAll", s.failed[1], s.failed[2])
        end
    end
    if result.status == "passed" then
        local ready = true
        for _, s in ipairs(c) do
            for _, h in ipairs(s.beforeEach) do
                local ok, e, tb = call(h)
                if not ok then
                    fail("beforeEach", e, tb)
                    ready = false
                    break
                end
            end
            if not ready then
                break
            end
        end
        if ready then
            local ok, e, tb = call(t.fn)
            if not ok then
                fail(nil, e, tb)
            end
        end
        for i = #c, 1, -1 do
            for _, h in ipairs(c[i].afterEach) do
                local ok, e, tb = call(h)
                if not ok then
                    fail("afterEach", e, tb)
                end
            end
        end
    end
    leave(c, nextIndex, fail)
    return result
end

-- A test abandoned by the runner (timed out): run its afterEach and the afterAll it owes.
function M.abort(index, nextIndex)
    local t = M.tests[index]
    local messages = {}
    local function fail(phase, e)
        messages[#messages + 1] = phase .. " failed: " .. errmsg(e)
    end
    local c = chain(t.suite)
    for i = #c, 1, -1 do
        for _, h in ipairs(c[i].afterEach) do
            local ok, e = call(h)
            if not ok then
                fail("afterEach", e)
            end
        end
    end
    leave(c, nextIndex, fail)
    return table.concat(messages, "\n")
end
