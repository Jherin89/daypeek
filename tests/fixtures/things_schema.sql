-- The subset of the Things 3 schema that `today` reads (Things 3.13+).
CREATE TABLE TMArea (uuid TEXT PRIMARY KEY, title TEXT);
CREATE TABLE TMTask (
    uuid TEXT PRIMARY KEY,
    title TEXT,
    type INTEGER,        -- 0 to-do, 1 project, 2 heading
    status INTEGER,      -- 0 open, 2 canceled, 3 completed
    trashed INTEGER,
    start INTEGER,       -- 0 inbox, 1 anytime/today, 2 someday
    startDate INTEGER,   -- packed date
    deadline INTEGER,    -- packed date
    project TEXT,
    area TEXT,
    todayIndex INTEGER,
    "index" INTEGER
);
