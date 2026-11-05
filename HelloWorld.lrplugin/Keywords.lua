--[[----------------------------------------------------------------------------
  Keywords.lua
  Writes the accepted keywords to the catalog, in one write block (one undo step).
  Keywords are only added; a photo's other keywords are left alone.
------------------------------------------------------------------------------]]

local Keywords = {}

-- How long to wait for the catalog if another write is in progress.
local WRITE_TIMEOUT = 30

-- Returns the keyword at `parts` ({ 'Wildlife', 'Bird' }), creating any missing level.
-- createKeyword with returnExisting matches an existing keyword whatever its case, so an
-- existing "Bird" is reused for "bird". Call inside a write block. `cache` is keyed by
-- the lowercased path.
local function findOrCreate(catalog, parts, cache)
	local parent, path = nil, ''
	for _, name in ipairs(parts) do
		path = path .. '>' .. name:lower()
		local keyword = cache[path]
		if not keyword then
			keyword = catalog:createKeyword(name, {}, true, parent, true)
			if not keyword then error('Could not create the keyword "' .. name .. '"') end
			cache[path] = keyword
		end
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
