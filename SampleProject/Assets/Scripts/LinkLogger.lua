-- Logs the id of a rich text <link> when it is clicked.
local LinkLogger = {}

function LinkLogger:OnLinkClicked(id)
    print("Link clicked: " .. id)
end

return LinkLogger
