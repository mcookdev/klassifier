--[[----------------------------------------------------------------------------
  Keywords.lua
  Writes the accepted keywords to the catalog, in one write block (one undo step) per
  Save All or Save Keywords.
  Keywords are only added; a photo's other keywords are left alone.
------------------------------------------------------------------------------]]

local LrLogger = import 'LrLogger'
local LrTasks  = import 'LrTasks'

local logger = LrLogger('Klassifier')
logger:enable('logfile')

local Keywords = {}

-- How long to wait for the catalog if another write is in progress.
local WRITE_TIMEOUT = 30

-- The child of `parent` (or top-level keyword, if `parent` is nil) named `name`, whatever
-- its case, or nil. Keywords in `created` are skipped (see findOrCreate).
local function findChild(catalog, parent, name, created)
	local siblings = parent and parent:getChildren() or catalog:getKeywords()
	local wanted = name:lower()
	for _, keyword in ipairs(siblings) do
		if not created[keyword] and keyword:getName():lower() == wanted then return keyword end
	end
	return nil
end

-- Returns the keyword at `parts` ({ 'Wildlife', 'Bird' }), creating any missing level. Each
-- level is looked up among its parent's children ourselves: createKeyword's returnExisting
-- can return a same-named keyword elsewhere in the tree (a top-level "eagle" for
-- Wildlife > Bird > Eagle). Call inside a write block. `cache` is keyed by the lowercased path.
--
-- Lightroom fails when a keyword created in the write block is asked anything (getParent
-- errors with "bad argument #2 to 'format'") until the block ends. So such keywords are
-- recorded in `created` (keyword -> true) and never called: messages use `parts`, nothing
-- is looked up under one (it has no children yet), and lookups skip them (the cache
-- already holds them by path).
local function findOrCreate(catalog, parts, cache, created)
	local parent, path = nil, ''
	for i, name in ipairs(parts) do
		path = path .. '>' .. name:lower()
		local where = i > 1 and ('under "' .. table.concat(parts, ' > ', 1, i - 1) .. '"') or 'at top level'
		local keyword = cache[path]
		if not keyword and not created[parent] then keyword = findChild(catalog, parent, name, created) end
		if not keyword then
			-- returnExisting = false: we already know no such child exists.
			local ok, result = LrTasks.pcall(catalog.createKeyword, catalog, name, {}, true, parent, false)
			if not ok then error('Could not create the keyword "' .. name .. '" ' .. where .. ': ' .. tostring(result)) end
			if not result then error('Lightroom refused to create the keyword "' .. name .. '" ' .. where) end
			logger:info('Created keyword "' .. name .. '" ' .. where)
			keyword = result
			created[keyword] = true
		end
		cache[path] = keyword
		parent = keyword
	end
	return parent
end

-- Adds each item's keywords (`pathsFor(item)`, a list of name lists) to all of its photos.
-- Returns the number of keyword assignments and of photos that got at least one, or nil if
-- the catalog stayed busy. An error inside the block discards all of its changes.
function Keywords.apply(catalog, items, pathsFor)
	local assigned, photos = 0, 0
	local status = catalog:withWriteAccessDo('Add suggested keywords', function()
		local cache, created = {}, {}
		for _, item in ipairs(items) do
			local paths = pathsFor(item)
			for _, parts in ipairs(paths) do
				local keyword = findOrCreate(catalog, parts, cache, created)
				for _, photo in ipairs(item.photos) do photo:addKeyword(keyword) end
			end
			if #paths > 0 then
				assigned = assigned + #paths * #item.photos
				photos = photos + #item.photos
			end
		end
	end, { timeout = WRITE_TIMEOUT })
	if status ~= 'executed' then return nil end
	return assigned, photos
end

return Keywords
