import java.io.File;
import java.lang.reflect.Field;
import java.security.MessageDigest;
import java.util.*;
import net.minecraft.src.*;

/** Dump golden: hash blok/heightmap/biome dari ChunkProviderGenerate asli b1.7.3. */
public class GenDump {
    /** FNV-1a 64-bit atas byte tak bertanda; sama dengan mc_core::selftest::fnv1a64. */
    static String hex(byte[] d) throws Exception {
        long h = 0xcbf29ce484222325L;
        for (byte b : d) { h ^= (b & 0xffL); h *= 0x100000001b3L; }
        return String.format("%016x", h);
    }
    static String hexD(double[] d) {
        long h = 0xcbf29ce484222325L;
        for (double v : d) {
            long bits = Double.doubleToLongBits(v);
            for (int i = 7; i >= 0; i--) { h ^= (bits >>> (8 * i)) & 0xffL; h *= 0x100000001b3L; }
        }
        return String.format("%016x", h);
    }
    static Object get(Object o, String n) throws Exception {
        Class<?> c = o.getClass();
        while (c != null) {
            try { Field f = c.getDeclaredField(n); f.setAccessible(true); return f.get(o); }
            catch (NoSuchFieldException e) { c = c.getSuperclass(); }
        }
        throw new NoSuchFieldException(n);
    }
    public static void main(String[] a) throws Exception {
        long seed = Long.parseLong(a[0]);
        File dir = java.nio.file.Files.createTempDirectory("mcgen").toFile();
        World w = new World(new SaveHandler(dir, "w", false), "w", seed, new WorldProviderSurface());
        ChunkProviderGenerate cp = new ChunkProviderGenerate(w, seed);
        int[][] cs = {{0,0},{-1,-1},{5,7},{-9,3},{20,-14}};
        for (int[] c : cs) {
            System.out.println("chunk " + c[0] + " " + c[1]);
            // Tahap-tahap antara (untuk melokalisasi beda): suhu, kelembapan, terrain mentah, setelah permukaan.
            java.util.Random rr = (java.util.Random) get(cp, "rand");
            rr.setSeed((long) c[0] * 341873128712L + (long) c[1] * 132897987541L);
            WorldChunkManager mgr = w.getWorldChunkManager();
            BiomeGenBase[] bio0 = mgr.loadBlockGeneratorData(null, c[0] * 16, c[1] * 16, 16, 16);
            String hTemp = hexD(mgr.temperature), hHum = hexD(mgr.humidity);
            byte[] st = new byte[32768];
            cp.generateTerrain(c[0], c[1], st, bio0, mgr.temperature);
            String hTer = hex(st);
            cp.replaceBlocksForBiome(c[0], c[1], st, bio0);
            String hSur = hex(st);
            System.out.println(" stages " + hTemp + " " + hHum + " " + hTer + " " + hSur);
            Chunk ch = cp.provideChunk(c[0], c[1]);
            byte[] hm = (byte[]) get(ch, "heightMap");
            NibbleArray sky = (NibbleArray) get(ch, "skylightMap");
            byte[] skd = (byte[]) get(sky, "data");
            // biome 16x16 (x + z*16) lewat WorldChunkManager
            BiomeGenBase[] bio = w.getWorldChunkManager().loadBlockGeneratorData(null, c[0]*16, c[1]*16, 16, 16);
            StringBuilder bs = new StringBuilder();
            for (BiomeGenBase b : bio) bs.append(b.biomeName.charAt(0)).append(b.biomeName.length()).append(',');
            int[] cnt = new int[256];
            for (byte b : ch.blocks) cnt[b & 255]++;
            StringBuilder cc = new StringBuilder();
            for (int i = 0; i < 256; i++) if (cnt[i] > 0) cc.append(i).append(':').append(cnt[i]).append(' ');
            System.out.println(" blocks " + hex(ch.blocks));
            System.out.println(" height " + hex(hm));
            System.out.println(" sky " + hex(skd));
            System.out.println(" biome " + hex(bs.toString().getBytes()));
            System.out.println(" counts " + cc.toString().trim());
        }
    }
}
