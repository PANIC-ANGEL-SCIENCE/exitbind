# Privacy gate source context

The repository-only `privacy_gate` integration test uses Python 3's standard
library to parse a bounded Python source as data when deciding whether one
credential-assignment occurrence is a computed group access. It never runs the
examined source. The standalone Cargo test consequently needs `python3` on
`PATH` to recognize this narrow form; a missing or failing interpreter retains
the original finding. Unsupported Python grammar, ambiguous source/history
context, size or time limits also retain the finding. This is a test dependency,
not a dependency of the Exitbind runtime. The bounded child transport currently
uses Unix sockets; other platforms retain the original finding and have not
been qualified for this exception.
