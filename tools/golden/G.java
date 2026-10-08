import net.minecraft.src.*;
import java.lang.reflect.*;
import java.util.*;

public class G {
  static sun.misc.Unsafe U;
  static void setField(Object o, String name, Object v) throws Exception {
    Field f = o.getClass().getDeclaredField(name);
    f.setAccessible(true);
    f.set(o, v);
  }
  static long fnv(byte[] b) {
    long h = 0xcbf29ce484222325L;
    for (byte x : b) { h ^= (x & 0xff); h *= 0x100000001b3L; }
    return h;
  }
  static int code(BiomeGenBase b) {
    if (b == BiomeGenBase.rainforest) return 0;
    if (b == BiomeGenBase.swampland) return 1;
    if (b == BiomeGenBase.seasonalForest) return 2;
    if (b == BiomeGenBase.forest) return 3;
    if (b == BiomeGenBase.savanna) return 4;
    if (b == BiomeGenBase.shrubland) return 5;
    if (b == BiomeGenBase.taiga) return 6;
    if (b == BiomeGenBase.desert) return 7;
    if (b == BiomeGenBase.plains) return 8;
    if (b == BiomeGenBase.iceDesert) return 9;
    if (b == BiomeGenBase.tundra) return 10;
    return 99;
  }
  public static void main(String[] a) throws Exception {
    Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
    uf.setAccessible(true);
    U = (sun.misc.Unsafe) uf.get(null);

    long[] seeds = {0xCAFEBABEL, 12345L, -4172144997902289642L};
    int[][] chunks = {{0,0},{-1,0},{3,-5},{7,7},{-9,12},{20,-20}};
    for (long seed : seeds) {
      Constructor<WorldChunkManager> c = WorldChunkManager.class.getDeclaredConstructor();
      c.setAccessible(true);
      WorldChunkManager cm = c.newInstance();
      setField(cm, "field_4194_e", new NoiseGeneratorOctaves2(new Random(seed * 9871L), 4));
      setField(cm, "field_4193_f", new NoiseGeneratorOctaves2(new Random(seed * 39811L), 4));
      setField(cm, "field_4192_g", new NoiseGeneratorOctaves2(new Random(seed * 543321L), 2));
      WorldProvider wp = (WorldProvider) U.allocateInstance(WorldProviderSurface.class);
      wp.worldChunkMgr = cm;
      World w = (World) U.allocateInstance(World.class);
      Field pf = World.class.getDeclaredField("worldProvider");
      U.putObject(w, U.objectFieldOffset(pf), wp);
      ChunkProviderGenerate p = new ChunkProviderGenerate(w, seed);
      Field rf = ChunkProviderGenerate.class.getDeclaredField("rand");
      rf.setAccessible(true);
      Random rand = (Random) rf.get(p);
      for (int[] ch : chunks) {
        int cx = ch[0], cz = ch[1];
        byte[] blocks = new byte[32768];
        rand.setSeed((long) cx * 341873128712L + (long) cz * 132897987541L);
        BiomeGenBase[] biomes = cm.loadBlockGeneratorData(null, cx * 16, cz * 16, 16, 16);
        double[] temps = cm.temperature;
        double[] hum = cm.humidity;
        byte[] bc = new byte[256];
        for (int i = 0; i < 256; i++) bc[i] = (byte) code(biomes[i]);
        p.generateTerrain(cx, cz, blocks, biomes, temps);
        long h1 = fnv(blocks);
        p.replaceBlocksForBiome(cx, cz, blocks, biomes);
        long h2 = fnv(blocks);
        System.out.println("CHUNK " + seed + " " + cx + " " + cz + " " + Long.toHexString(fnv(bc)) + " "
          + Long.toHexString(h1) + " " + Long.toHexString(h2) + " "
          + Long.toHexString(Double.doubleToLongBits(temps[0])) + " " + Long.toHexString(Double.doubleToLongBits(hum[0])) + " "
          + Long.toHexString(Double.doubleToLongBits(temps[255])) + " " + Long.toHexString(Double.doubleToLongBits(hum[255])));
      }
    }
    // raw noise vectors
    Random r = new Random(0xCAFEBABEL);
    NoiseGeneratorOctaves n = new NoiseGeneratorOctaves(r, 4);
    double[] o3 = n.generateNoiseOctaves(null, 10.0, 20.0, 30.0, 2, 3, 2, 0.05, 0.07, 0.09);
    System.out.println("OCT3D " + Arrays.toString(o3));
    double[] o2 = n.generateNoiseOctaves(null, -7.0, 10.0, 5.0, 3, 1, 2, 0.31, 1.0, 0.17);
    System.out.println("OCT2D " + Arrays.toString(o2));
    System.out.println("AFTER " + r.nextInt(1000) + " " + r.nextInt(64) + " " + r.nextLong() + " " + r.nextDouble());
    Random r2 = new Random(777L);
    NoiseGeneratorOctaves2 s = new NoiseGeneratorOctaves2(r2, 3);
    double[] sx = s.func_4112_a(null, 12.0, -30.0, 3, 2, 0.025, 0.05, 0.25);
    System.out.println("SIMPLEX " + Arrays.toString(sx));
    Random r3 = new Random(-99L);
    System.out.println("RAND " + r3.nextInt() + " " + r3.nextInt(5) + " " + r3.nextInt(16) + " " + r3.nextInt(100) + " " + r3.nextLong() + " " + r3.nextFloat() + " " + r3.nextBoolean() + " " + r3.nextDouble());
  }
}
