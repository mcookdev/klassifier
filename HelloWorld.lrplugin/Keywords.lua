--[[----------------------------------------------------------------------------
  Keywords.lua
  Writes the accepted keywords to the catalog, in one write block (one undo step).
  Keywords are only added; a photo's other keywords are left alone.
------------------------------------------------------------------------------]]

local LrLogger = import 'LrLogger'
local LrTasks  = import 'LrTasks'

local logger = LrLogger('HelloWorld')
logger:enable('logfile')

local Keywords = {}

local function describeKeyword(keyword)
	local parent = keyword:getParent()
	return string.format('"%s" (%s)', keyword:getName(), parent and ('under "' .. parent:getName() .. '"') or 'top level')
end

-- How long to wait for the catalog if another write is in progress.
local WRITE_TIMEOUT = 30

-- The child of `parent` (or top-level keyword, if `parent` is nil) named `name`, whatever
-- its case, or nil.
local function findChild(catalog, parent, name)
	local siblings = parent and parent:getChildren() or catalog:getKeywords()
	local wanted = name:lower()
	for _, keyword in ipairs(siblings) do
		if keyword:getName():lower() == wanted then return keyword end
	end
	return nil
end

-- Returns the keyword at `parts` ({ 'Wildlife', 'Bird' }), creating any missing level. Each
-- level is looked up among its parent's children ourselves: createKeyword's returnExisting
-- can return a same-named keyword elsewhere in the tree (a top-level "eagle" for
-- Wildlife > Bird > Eagle). Call inside a write block. `cache` is keyed by the lowercased path.
local function findOrCreate(catalog, parts, cache)
	local parent, path = nil, ''
	for _, name in ipairs(parts) do
		path = path .. '>' .. name:lower()
		local keyword = cache[path] or findChild(catalog, parent, name)
		local where = parent and ('under "' .. parent:getName() .. '"') or 'at top level'
		if not keyword then
			-- returnExisting = false: we already know no such child exists.
			local ok, result = LrTasks.pcall(catalog.createKeyword, catalog, name, {}, true, parent, false)
			if not ok then error('Could not create the keyword "' .. name .. '" ' .. where .. ': ' .. tostring(result)) end
			if not result then error('Lightroom refused to create the keyword "' .. name .. '" ' .. where) end
			logger:info('Created keyword ' .. describeKeyword(result))
			keyword = result
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
		local cache = {}
		for _, item in ipairs(items) do
			local paths = pathsFor(item)
			for _, parts in ipairs(paths) do
				local keyword = findOrCreate(catalog, parts, cache)
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
