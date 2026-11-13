-- Wireshark / TShark Lua Dissector for Samsung LOKE / Odin Wire Protocol
-- Author: Rabeeh Fouz Ahammad VK
-- License: MIT
-- Usage: tshark -X lua_script:tools/wireshark/exynos_loke.lua -r capture.pcapng

local loke_proto = Proto("loke", "Samsung LOKE / Odin USB Protocol")

-- Protocol Fields
local f_magic = ProtoField.string("loke.magic", "Handshake Magic")
local f_cmd = ProtoField.uint32("loke.cmd", "Command Opcode", base.HEX, {
    [0x64] = "CMD_SESSION_SETUP (0x64)",
    [0x65] = "CMD_PIT_TRANSFER (0x65)",
    [0x66] = "CMD_FILE_TRANSFER (0x66)",
    [0x67] = "CMD_SESSION_END (0x67)",
})
local f_subcmd = ProtoField.uint32("loke.subcmd", "Subcommand ID", base.HEX)
local f_arg0 = ProtoField.uint32("loke.arg0", "Argument 0", base.HEX)
local f_arg1 = ProtoField.uint32("loke.arg1", "Argument 1", base.HEX)
local f_arg2 = ProtoField.uint32("loke.arg2", "Argument 2", base.HEX)
local f_arg3 = ProtoField.uint32("loke.arg3", "Argument 3", base.HEX)
local f_str_arg = ProtoField.string("loke.string_arg", "String Parameter")
local f_ack_status = ProtoField.uint32("loke.ack_status", "ACK Status Code", base.HEX, {
    [0x00] = "STATUS_OK (0x00)",
    [0x01] = "STATUS_BUSY / FAIL (0x01)",
    [0xFFFFFFFF] = "STATUS_ERROR (-1)",
})
local f_ack_data = ProtoField.uint32("loke.ack_data", "ACK Additional Data", base.HEX)

loke_proto.fields = {
    f_magic, f_cmd, f_subcmd, f_arg0, f_arg1, f_arg2, f_arg3, f_str_arg,
    f_ack_status, f_ack_data
}

function loke_proto.dissector(buffer, pinfo, tree)
    local length = buffer:len()
    if length == 0 then return end

    pinfo.cols.protocol = "LOKE"

    -- 1. Initial Handshake Check (4-byte ASCII: ODIN or LOKE)
    if length == 4 then
        local magic = buffer(0, 4):string()
        if magic == "ODIN" or magic == "LOKE" then
            local subtree = tree:add(loke_proto, buffer(), "Samsung LOKE Handshake: " .. magic)
            subtree:add(f_magic, buffer(0, 4))
            pinfo.cols.info = "Handshake: " .. magic
            return
        end
    end

    -- 2. Device Status / ACK Response (8 bytes: [status: u32_le, data: u32_le])
    if length == 8 then
        local subtree = tree:add(loke_proto, buffer(), "Samsung LOKE ACK Frame")
        local status = buffer(0, 4):le_uint()
        local data = buffer(4, 4):le_uint()
        subtree:add_le(f_ack_status, buffer(0, 4))
        subtree:add_le(f_ack_data, buffer(4, 4))
        pinfo.cols.info = string.format("LOKE ACK [Status: 0x%08X, Data: 0x%08X]", status, data)
        return
    end

    -- 3. Host Control Frame (1024 bytes)
    if length == 1024 then
        local cmd = buffer(0, 4):le_uint()
        local subcmd = buffer(4, 4):le_uint()
        local subtree = tree:add(loke_proto, buffer(), string.format("Samsung LOKE Control Frame (Cmd: 0x%02X, Subcmd: 0x%02X)", cmd, subcmd))
        subtree:add_le(f_cmd, buffer(0, 4))
        subtree:add_le(f_subcmd, buffer(4, 4))
        subtree:add_le(f_arg0, buffer(8, 4))
        subtree:add_le(f_arg1, buffer(12, 4))
        subtree:add_le(f_arg2, buffer(16, 4))
        subtree:add_le(f_arg3, buffer(20, 4))

        -- Check for null-terminated string at offset 24
        local str_val = buffer(24):stringz()
        if str_val and #str_val > 0 then
            subtree:add(f_str_arg, buffer(24, #str_val), str_val)
            pinfo.cols.info = string.format("LOKE CMD 0x%02X:0x%02X (\"%s\")", cmd, subcmd, str_val)
        else
            pinfo.cols.info = string.format("LOKE CMD 0x%02X:0x%02X", cmd, subcmd)
        end
        return
    end

    -- 4. Bulk Payload Stream (PIT, Sparse chunk, or Raw partition bytes)
    local subtree = tree:add(loke_proto, buffer(), string.format("Samsung LOKE Bulk Data (%d bytes)", length))
    pinfo.cols.info = string.format("LOKE Bulk Data (%d bytes)", length)
end

-- Register dissector for USB bulk transfer heuristic
local usb_bulk_table = DissectorTable.get("usb.bulk")
if usb_bulk_table then
    usb_bulk_table:add(0xFFFF, loke_proto)
end
