# Golden vectors from the real b1.7.3 classes

`G.java` runs the decompiled classes in `minecraft/bin` (Random, noise,
WorldChunkManager, ChunkProviderGenerate) and prints reference values.
`golden.txt` is its output; the Rust tests in `mc-rs` embed those numbers.

Regenerate (JDK with `jdk.compiler`, e.g. Java 21):

    mkdir -p out res/lang && : > res/lang/en_US.lang && : > res/lang/stats_US.lang
    java -m jdk.compiler/com.sun.tools.javac.Main -nowarn -cp minecraft/bin -d out tools/golden/G.java
    java -cp out:res:minecraft/bin G > tools/golden/golden.txt

(The empty `lang` files stop `StringTranslate` failing on a missing resource;
the harmless "achievements" stack trace on stderr is expected.)

Add a new section to `G.java` for every new port (caves, trees, ...), then
copy its numbers into the matching Rust test.
