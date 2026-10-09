#pragma once
// CommandRegistry: every menu action is a named command with string arguments, so the same entry serves the menu, the command line
// (`--cmd "name args"`), script files (`--script file`) and tests. Tools register their own commands next to the editor's.
//
// Script files: one command per line, `#` starts a comment, arguments split on spaces (double quotes group). A line starting with `-`
// may fail without stopping the script. `frames N` pauses the script for N rendered frames.
#include <functional>
#include <map>
#include <string>
#include <vector>

namespace wce {

struct CommandResult {
    bool ok = true;
    std::string message;
    static CommandResult success(std::string m = {}) { return {true, std::move(m)}; }
    static CommandResult fail(std::string m) { return {false, std::move(m)}; }
};

class CommandRegistry {
public:
    using Args = std::vector<std::string>;
    using Fn = std::function<CommandResult(const Args&)>;

    struct Entry {
        std::string name, usage, help;
        Fn fn;
    };

    // Replaces an earlier registration of the same name.
    void add(const std::string& name, const std::string& usage, const std::string& help, Fn fn);
    CommandResult run(const std::string& line);            // tokenizes and runs one command
    CommandResult run(const std::string& name, const Args& args);
    const Entry* find(const std::string& name) const;
    std::vector<const Entry*> entries() const;             // sorted by name

    static Args tokenize(const std::string& line);          // quotes group, # comments

    // Set by the `frames` command: the script runner waits this many rendered frames before the next line.
    int wait_frames = 0;

private:
    std::map<std::string, Entry> entries_;
};

class ScriptRunner {
public:
    void load_text(const std::string& text);
    bool load_file(const std::string& path, std::string* error);
    // Inserts the file's lines right after the current position (the `script` command), so the rest of the running script still follows.
    bool include_file(const std::string& path, std::string* error);
    bool done() const { return next_ >= lines_.size() && wait_ == 0; }
    bool failed() const { return failed_; }
    const std::string& error() const { return error_; }
    // Runs lines until one asks to wait (or the script ends or fails). Call once per rendered frame; returns false once the script has failed.
    bool step(CommandRegistry& registry);

private:
    std::vector<std::string> lines_;
    size_t next_ = 0;
    int wait_ = 0;
    bool failed_ = false;
    std::string error_;
};

} // namespace wce
