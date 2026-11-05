--[[----------------------------------------------------------------------------
  Review.lua
  The review window: a paged grid of the classified photos with their suggested
  keywords, and Accept All / Cancel.

  An item is one classified file:
    { path, name, photos = { LrPhoto... },   -- photos sharing the file (virtual copies)
      candidates = { label... },             -- the reported labels, best first
      suggested = { [label] = true },        -- the classifier's suggestions
      chosen = nil | { [label] = true } }    -- the user's choice, once edited
------------------------------------------------------------------------------]]

local LrBinding = import 'LrBinding'
local LrDialogs = import 'LrDialogs'
local LrTasks   = import 'LrTasks'
local LrView    = import 'LrView'

local Review = {}

local COLUMNS, ROWS = 4, 3
local PER_PAGE = COLUMNS * ROWS
local THUMB = 150
-- File names longer than this are shortened in the middle to fit under the thumbnail.
local NAME_CHARS = 24

local function plural(n, word)
	return string.format('%d %s%s', n, word, n == 1 and '' or 's')
end

-- The labels to write for `item`: its chosen ones if edited, else the suggested ones,
-- in candidate order.
function Review.keywordsFor(item)
	local set = item.chosen or item.suggested
	local labels = {}
	for _, label in ipairs(item.candidates) do
		if set[label] then labels[#labels + 1] = label end
	end
	return labels
end

-- Keyword assignments and photos that would get at least one, over all items.
function Review.totals(items)
	local keywords, photos = 0, 0
	for _, item in ipairs(items) do
		local n = #Review.keywordsFor(item)
		if n > 0 then
			keywords = keywords + n * #item.photos
			photos = photos + #item.photos
		end
	end
	return keywords, photos
end

function Review.itemStatus(item)
	local n = #Review.keywordsFor(item)
	if item.chosen then return 'edited: ' .. plural(n, 'keyword') end
	if n == 0 then return 'no suggestions' end
	return plural(n, 'keyword')
end

local function shortName(name)
	if #name <= NAME_CHARS then return name end
	local keep = math.floor((NAME_CHARS - 3) / 2)
	return name:sub(1, keep) .. '...' .. name:sub(-keep)
end

local function pageCount(items)
	return math.max(1, math.ceil(#items / PER_PAGE))
end

local function totalsLine(items)
	local keywords, photos = Review.totals(items)
	if keywords == 0 then
		return plural(#items, 'photo') .. ' to review. No keywords to add.'
	end
	return string.format('%s to review. %s to add on %s.',
		plural(#items, 'photo'), plural(keywords, 'keyword'), plural(photos, 'photo'))
end

-- Shows page `page` of `items` in the slots, and updates the paging and totals text.
local function fillPage(props, items, page)
	local pages = pageCount(items)
	props.page = page
	for n = 1, PER_PAGE do
		local item = items[(page - 1) * PER_PAGE + n]
		local key = 's' .. n .. '_'
		if item then
			props[key .. 'photo'] = item.photos[1]
			props[key .. 'name'] = shortName(item.name)
			props[key .. 'status'] = Review.itemStatus(item)
		else
			-- The thumbnail keeps its last photo but is hidden.
			props[key .. 'name'] = ''
			props[key .. 'status'] = ''
		end
		props[key .. 'visible'] = item ~= nil
	end
	props.pageText = string.format('Page %d of %d', page, pages)
	props.prevEnabled = page > 1
	props.nextEnabled = page < pages
	props.totals = totalsLine(items)
end

-- Shows the review window for `items` (at least one). `notes` is extra header text (skipped
-- files, cancel). `showDetails`, if given, adds a Details... button that calls it (it may
-- show a modal dialog). Returns 'ok' (Accept All) or 'cancel'.
function Review.show(context, items, notes, showDetails)
	local f = LrView.osFactory()
	local bind = LrView.bind
	local props = LrBinding.makePropertyTable(context)
	-- Hidden slots need a photo before the first page is filled.
	for n = 1, PER_PAGE do props['s' .. n .. '_photo'] = items[1].photos[1] end
	fillPage(props, items, 1)

	local grid = { spacing = f:control_spacing() }
	for r = 1, ROWS do
		local row = { spacing = f:control_spacing() }
		for c = 1, COLUMNS do
			-- visible works on controls, not on a column (spike S1), so each control gets it.
			local key = 's' .. ((r - 1) * COLUMNS + c) .. '_'
			local visible = bind(key .. 'visible')
			row[#row + 1] = f:column {
				width = THUMB,
				f:catalog_photo { photo = bind(key .. 'photo'), width = THUMB, height = THUMB, visible = visible },
				f:static_text { title = bind(key .. 'name'), width = THUMB, visible = visible },
				f:static_text { title = bind(key .. 'status'), width = THUMB, visible = visible },
			}
		end
		grid[#grid + 1] = f:row(row)
	end

	local top = {
		f:push_button {
			title = 'Accept All',
			action = function(button) LrDialogs.stopModalWithResult(button, 'ok') end,
		},
		f:push_button {
			title = 'Cancel',
			action = function(button) LrDialogs.stopModalWithResult(button, 'cancel') end,
		},
	}
	if showDetails then
		top[#top + 1] = f:push_button {
			title = 'Details...',
			action = function() LrTasks.startAsyncTask(showDetails) end,
		}
	end

	local contents = f:column {
		bind_to_object = props,
		spacing = f:control_spacing(),
		f:row(top),
		f:static_text { title = bind 'totals', fill_horizontal = 1 },
		f:static_text { title = notes, height_in_lines = select(2, notes:gsub('\n', '')) + 1, fill_horizontal = 1 },
		f:column(grid),
		f:row {
			f:push_button {
				title = '< Prev',
				enabled = bind 'prevEnabled',
				action = function() fillPage(props, items, props.page - 1) end,
			},
			f:static_text { title = bind 'pageText', width_in_chars = 14, alignment = 'center' },
			f:push_button {
				title = 'Next >',
				enabled = bind 'nextEnabled',
				action = function() fillPage(props, items, props.page + 1) end,
			},
		},
	}

	return LrDialogs.presentModalDialog {
		title = 'Review keywords',
		contents = contents,
		actionVerb = 'Accept All',
		cancelVerb = 'Cancel',
	}
end

return Review
