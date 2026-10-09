#include "editor/command_registry.h"

#include "foundation/log.h"

#include <fstream>
#include <sstream>

namespace wce {

CommandRegistry::Args CommandRegistry::tokenize(const std::string& line) {
    Args out;
    std::string cur;
    bool in_quote = false, have = false;
    for (char c : line) {
        if (in_quote) {
            if (c == '"') in_quote = false;
            else cur += c;
        } else if (c == '"') {
            in_quote = have = true;
        } else if (c == ' ' || c == '\t' || c == '\r') {
            if (have) { out.push_back(cur); cur.clear(); have = false; }
        } else if (c == '#' && !have) {
            break;
        } else {
            cur += c;
            have = true;
        }
    }
    if (have) out.push_back(cur);
    return out;
}

void CommandRegistry::add(const std::string& name, const std::string& usage, const std::string& help, Fn fn) {
    entries_[name] = Entry{name, usage, help, std::move(fn)};
}

const CommandRegistry::Entry* CommandRegistry::find(const std::string& name) const {
    auto it = entries_.find(name);
    return it == entries_.end() ? nullptr : &it->second;
}

std::vector<const CommandRegistry::Entry*> CommandRegistry::entries() const {
    std::vector<const Entry*> out;
    for (const auto& [n, e] : entries_) out.push_back(&e);
    return out;
}

CommandResult CommandRegistry::run(const std::string& name, const Args& args) {
    const Entry* e = find(name);
    if (!e) return CommandResult::fail("unknown command '" + name + "'");
    try {
        return e->fn(args);
    } catch (const std::exception& ex) {
        return CommandResult::fail(name + ": " + ex.what());
    }
}

CommandResult CommandRegistry::run(const std::string& line) {
    Args t = tokenize(line);
    if (t.empty()) return CommandResult::success();
    const std::string name = t[0];
    t.erase(t.begin());
    return run(name, t);
}

void ScriptRunner::load_text(const std::string& text) {
    lines_.clear();
    std::istringstream is(text);
    std::string l;
    while (std::getline(is, l)) lines_.push_back(l);
    next_ = 0;
    wait_ = 0;
    failed_ = false;
    error_.clear();
}

bool ScriptRunner::load_file(const std::string& path, std::string* error) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        if (error) *error = "cannot open script " + path;
        return false;
    }
    std::stringstream ss;
    ss << f.rdbuf();
    load_text(ss.str());
    return true;
}

bool ScriptRunner::include_file(const std::string& path, std::string* error) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        if (error) *error = "cannot open script " + path;
        return false;
    }
    std::vector<std::string> add;
    std::string l;
    while (std::getline(f, l)) add.push_back(l);
    lines_.insert(lines_.begin() + std::ptrdiff_t(next_), add.begin(), add.end());
    return true;
}

bool ScriptRunner::step(CommandRegistry& registry) {
    if (failed_) return false;
    if (wait_ > 0) {
        --wait_;
        return true;
    }
    while (next_ < lines_.size()) {
        std::string line = lines_[next_++];
        const size_t first = line.find_first_not_of(" \t\r");
        if (first == std::string::npos || line[first] == '#') continue;
        bool optional = false;
        if (line[first] == '-') {
            optional = true;
            line.erase(0, first + 1);
        }
        registry.wait_frames = 0;
        registry.repeat = false;
        CommandResult r = registry.run(line);
        if (!r.message.empty()) Log::info(line + " -> " + r.message);
        if (!r.ok && !optional) {
            failed_ = true;
            error_ = "script line " + std::to_string(next_) + ": " + line + ": " + r.message;
            Log::error(error_);
            return false;
        }
        if (registry.repeat) {
            --next_;
            registry.repeat = false;
            return true;
        }
        if (registry.wait_frames > 0) {
            wait_ = registry.wait_frames - 1;   // this frame counts as the first waited
            registry.wait_frames = 0;
            return true;
        }
    }
    return true;
}

} // namespace wce
