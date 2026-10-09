import net.minecraft.src.*;
import java.lang.reflect.*;
import java.util.*;

public class G {
  // {seed index, chunk x, chunk z}: taiga + snow, forest, lava lake, dungeon, pumpkin, mushrooms, cactus, clay, big trees.
  static final int[][] POP_CASES = {{0,-48,-40},{0,-32,-40},{0,-32,16},{0,24,-24},{0,16,16},{0,24,32},{1,3,0},{1,-3,0},{2,-6,3},{2,-3,-6},{2,3,-6}};
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

  /** World stand-in for populate: the 2x2 chunks at (cx0, cz0), air outside, writes outside dropped.
   *  Light/height use the same column model as mc-rs populate.rs (see its module docs). */
  static class FakeWorld extends World {
    long seed; int cx0, cz0; byte[][] c = new byte[4][]; NibbleArray[] md = new NibbleArray[4]; WorldChunkManager cm;
    FakeWorld() { super((ISaveHandler) null, "x", (WorldProvider) null, 0L); }
    int slot(int x, int y, int z) {
      if (y < 0 || y >= 128) return -1;
      int i = (x >> 4) - cx0, j = (z >> 4) - cz0;
      if (i < 0 || i > 1 || j < 0 || j > 1) return -1;
      return i * 2 + j;
    }
    int ix(int x, int y, int z) { return ((x & 15) << 11) | ((z & 15) << 7) | y; }
    public int getBlockId(int x, int y, int z) { int s = slot(x, y, z); return s < 0 || c[s] == null ? 0 : c[s][ix(x, y, z)] & 255; }
    // Chunk.setBlockID: a different id resets the metadata nibble to 0; same id changes nothing.
    public boolean setBlock(int x, int y, int z, int id) {
      int s = slot(x, y, z);
      if (s >= 0 && c[s] != null) {
        int i = ix(x, y, z);
        if ((c[s][i] & 255) != id) { c[s][i] = (byte) id; md[s].setNibble(x & 15, y, z & 15, 0); }
      }
      return true;
    }
    public boolean setBlockWithNotify(int x, int y, int z, int id) { return setBlock(x, y, z, id); }
    // Chunk.setBlockIDWithMetadata: id and nibble are both written.
    public boolean setBlockAndMetadata(int x, int y, int z, int id, int m) {
      int s = slot(x, y, z);
      if (s >= 0 && c[s] != null) { c[s][ix(x, y, z)] = (byte) id; md[s].setNibble(x & 15, y, z & 15, m); }
      return true;
    }
    public boolean setBlockAndMetadataWithNotify(int x, int y, int z, int id, int m) { return setBlockAndMetadata(x, y, z, id, m); }
    public int getBlockMetadata(int x, int y, int z) { int s = slot(x, y, z); return s < 0 || md[s] == null ? 0 : md[s].getNibble(x & 15, y, z & 15); }
    public void scheduleLightingUpdate(EnumSkyBlock t, int a, int b, int c, int d, int e, int f) {}
    public void neighborLightPropagationChanged(EnumSkyBlock t, int a, int b, int c, int d) {}
    public long getRandomSeed() { return seed; }
    public WorldChunkManager getWorldChunkManager() { return cm; }
    public TileEntity getBlockTileEntity(int x, int y, int z) {
      int id = getBlockId(x, y, z);
      return id == 54 ? new TileEntityChest() : id == 52 ? new TileEntityMobSpawner() : null;
    }
    public int getHeightValue(int x, int z) {
      if (slot(x, 0, z) < 0) return 0;
      int y = 127;
      while (y > 0 && Block.lightOpacity[getBlockId(x, y - 1, z)] == 0) y--;
      return y;
    }
    public boolean canBlockSeeTheSky(int x, int y, int z) { return y >= getHeightValue(x, z); }
    int sky(int x, int y, int z) {
      if (y < 0) return 0;
      int l = 15;
      for (int yy = 127; yy >= Math.min(y, 127); yy--) { l -= Block.lightOpacity[getBlockId(x, yy, z)]; if (l <= 0) return 0; }
      return l;
    }
    public int getFullBlockLightValue(int x, int y, int z) { return sky(x, y, z); }
    public int getSavedLightValue(EnumSkyBlock t, int x, int y, int z) { return t == EnumSkyBlock.Sky ? sky(x, y, z) : 0; }
    public int findTopSolidBlock(int x, int z) {
      for (int y = 127; y > 0; y--) {
        int id = getBlockId(x, y, z);
        Material m = id == 0 ? Material.air : Block.blocksList[id].blockMaterial;
        if (m.getIsSolid() || m.getIsLiquid()) return y + 1;
      }
      return -1;
    }
  }

  static void tables() {
    int[] ids = {0,1,2,3,4,5,7,8,9,10,11,12,13,14,15,16,17,18,21,24,31,32,37,38,39,40,48,52,54,56,60,73,78,79,81,82,83,86};
    for (int id : ids) {
      Block b = Block.blocksList[id];
      boolean solid = id == 0 ? false : b.blockMaterial.isSolid();
      boolean liquid = id == 0 ? false : b.blockMaterial.getIsLiquid();
      System.out.println("TAB " + id + " " + (Block.opaqueCubeLookup[id] ? 1 : 0) + " " + Block.lightOpacity[id] + " " + (solid ? 1 : 0) + " " + (liquid ? 1 : 0));
    }
  }

  static void populateSection(long[] seeds, int[][] cases, boolean explore) throws Exception {
    int si = -1;
    Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
    // Springs would start to flow (block ticks): the Rust port does not, so make the tick a no-op.
    for (int id : new int[]{8, 10}) {
      Block.blocksList[id] = null;
      Block.blocksList[id] = new Block(id, id == 8 ? Material.water : Material.lava) {
        public boolean isOpaqueCube() { return false; }
        public void updateTick(World w, int x, int y, int z, Random r) {}
      };
      Block.lightOpacity[id] = id == 8 ? 3 : 255;
    }
    for (long seed : seeds) {
      si++;
      Constructor<WorldChunkManager> c = WorldChunkManager.class.getDeclaredConstructor();
      c.setAccessible(true);
      WorldChunkManager cm = c.newInstance();
      setField(cm, "field_4194_e", new NoiseGeneratorOctaves2(new Random(seed * 9871L), 4));
      setField(cm, "field_4193_f", new NoiseGeneratorOctaves2(new Random(seed * 39811L), 4));
      setField(cm, "field_4192_g", new NoiseGeneratorOctaves2(new Random(seed * 543321L), 2));
      WorldProvider wp = (WorldProvider) U.allocateInstance(WorldProviderSurface.class);
      wp.worldChunkMgr = cm;
      FakeWorld w = (FakeWorld) U.allocateInstance(FakeWorld.class);
      U.putObject(w, U.objectFieldOffset(World.class.getDeclaredField("worldProvider")), wp);
      w.seed = seed; w.cm = cm; w.c = new byte[4][]; w.md = new NibbleArray[4];
      ChunkProviderGenerate p = new ChunkProviderGenerate(w, seed);
      for (int[] ch0 : cases) {
        if (!explore && ch0[0] != si) continue;
        int cx = explore ? ch0[0] : ch0[1], cz = explore ? ch0[1] : ch0[2];
        w.cx0 = cx; w.cz0 = cz;
        int[][] at = {{cx, cz}, {cx, cz + 1}, {cx + 1, cz}, {cx + 1, cz + 1}};
        StringBuilder raw = new StringBuilder("RAW " + seed + " " + cx + " " + cz);
        for (int i = 0; i < 4; i++) {
          Chunk ck = p.provideChunk(at[i][0], at[i][1]);
          w.c[i] = ck.blocks.clone();
          w.md[i] = new NibbleArray(ck.data.data.clone());
          raw.append(" ").append(Long.toHexString(fnv(w.c[i])));
        }
        System.out.println(raw);
        p.populate(p, cx, cz);
        StringBuilder pop = new StringBuilder("POP " + seed + " " + cx + " " + cz);
        for (int i = 0; i < 4; i++) pop.append(" ").append(Long.toHexString(fnv(w.c[i])));
        System.out.println(pop);
        // META: FNV of each chunk's packed metadata nibbles (NibbleArray.data = the McRegion Data tag layout).
        StringBuilder meta = new StringBuilder("META " + seed + " " + cx + " " + cz);
        for (int i = 0; i < 4; i++) meta.append(" ").append(Long.toHexString(fnv(w.md[i].data)));
        System.out.println(meta);
        if (explore) {
          boolean[] seen = new boolean[256];
          for (byte[] b : w.c) for (byte x : b) seen[x & 255] = true;
          StringBuilder s = new StringBuilder("IDS " + seed + " " + cx + " " + cz + " " + code(cm.getBiomeGenAt(cx * 16 + 16, cz * 16 + 16)) + ":");
          for (int i = 0; i < 256; i++) if (seen[i] && (i == 17 || i == 18 || i == 52 || i == 54 || i == 78 || i == 81 || i == 82 || i == 83 || i == 86 || i == 31 || i == 37 || i == 38 || i == 39 || i == 40 || i == 48 || i == 8 || i == 10 || i == 11)) s.append(" ").append(i);
          System.out.println(s);
        }
      }
    }
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
    tables();
    String ex = System.getProperty("explore");
    if (ex != null) {
      java.util.List<int[]> cs = new java.util.ArrayList<>();
      for (int x = -48; x < 48; x += 8) for (int z = -48; z < 48; z += 8) cs.add(new int[]{x, z});
      populateSection(seeds, cs.toArray(new int[0][]), true);
    } else {
      populateSection(seeds, POP_CASES, false);
    }
  }
}
