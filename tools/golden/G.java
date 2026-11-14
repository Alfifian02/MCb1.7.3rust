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
  static String fb(float f) { return Integer.toHexString(Float.floatToIntBits(f)); }

  /** Sky + weather reference values: celestial angle, fog colour, sunrise glow, star brightness, skylight with
   *  rain/thunder, the updateWeather timer/strength state machine, and the star quads of RenderGlobal.renderStars. */
  static void sky() throws Exception {
    WorldProvider wp = new WorldProviderSurface(); // a real constructor: allocateInstance would skip the sunrise colour array
    World w = (World) U.allocateInstance(World.class);
    Field pf = World.class.getDeclaredField("worldProvider");
    U.putObject(w, U.objectFieldOffset(pf), wp);
    WorldInfo wi = new WorldInfo(0L, "x");
    setField(w, "worldInfo", wi);
    long[] times = {0, 1000, 5000, 6000, 11999, 12000, 13000, 17999, 18000, 22500, 23999, 24000, 30000};
    for (long t : times) {
      float partial = 0.25f;
      float ang = wp.calculateCelestialAngle(t, partial);
      Vec3D fog = wp.func_4096_a(ang, partial);
      float[] sr = wp.calcSunriseSunsetColors(ang, partial);
      wi.setWorldTime(t);
      System.out.println("CEL " + t + " " + fb(ang) + " " + fb((float) fog.xCoord) + " " + fb((float) fog.yCoord) + " " + fb((float) fog.zCoord) + " "
        + (sr == null ? "none" : fb(sr[0]) + " " + fb(sr[1]) + " " + fb(sr[2]) + " " + fb(sr[3])) + " " + fb(w.getStarBrightness(partial)));
    }
    float[][] rt = {{0, 0}, {1, 0}, {1, 1}, {0.5f, 0.5f}, {0.3f, 1}, {0, 1}};
    for (long t : new long[]{0, 3000, 6000, 9000, 12000, 14000, 17000, 18000, 21000, 23000}) {
      wi.setWorldTime(t);
      for (float[] x : rt) {
        setField(w, "prevRainingStrength", x[0]); setField(w, "rainingStrength", x[0]);
        setField(w, "prevThunderingStrength", x[1]); setField(w, "thunderingStrength", x[1]);
        System.out.println("SUBL " + t + " " + fb(w.func_27162_g(1.0f)) + " " + fb(w.func_27166_f(1.0f)) + " " + w.calculateSkylightSubtracted(1.0f));
      }
    }
    // updateWeather: a fresh world (rainTime = thunderTime = 0), java.util.Random seeded like the Rust side.
    Method uw = World.class.getDeclaredMethod("updateWeather");
    uw.setAccessible(true);
    for (long seed : new long[]{0xCAFEBABEL, 12345L}) {
      World ww = (World) U.allocateInstance(World.class);
      U.putObject(ww, U.objectFieldOffset(pf), wp);
      WorldInfo wwi = new WorldInfo(0L, "x");
      setField(ww, "worldInfo", wwi);
      ww.rand = new Random(seed);
      long h = 0xcbf29ce484222325L;
      int rainStarts = 0, thunderStarts = 0;
      boolean pr = false, pt = false;
      for (int tick = 1; tick <= 1000000; tick++) {
        uw.invoke(ww);
        float rs = ww.func_27162_g(1.0f), ts = ww.func_27166_f(1.0f);
        long[] v = {wwi.getRaining() ? 1 : 0, wwi.getThundering() ? 1 : 0, wwi.getRainTime(), wwi.getThunderTime(), Float.floatToIntBits(rs), Float.floatToIntBits(ts)};
        for (long x : v) { h ^= x & 0xffffffffL; h *= 0x100000001b3L; }
        if (wwi.getRaining() && !pr) rainStarts++;
        if (wwi.getThundering() && !pt) thunderStarts++;
        pr = wwi.getRaining(); pt = wwi.getThundering();
        if (tick == 1 || tick == 50000 || tick == 400000 || tick == 1000000)
          System.out.println("WEAT " + seed + " " + tick + " " + v[0] + " " + v[1] + " " + v[2] + " " + v[3] + " " + fb(rs) + " " + fb(ww.func_27162_g(0.5f)));
      }
      System.out.println("WEAH " + seed + " " + Long.toHexString(h) + " " + rainStarts + " " + thunderStarts);
    }
    // RenderGlobal.renderStars, verbatim, collecting the quad corners instead of calling the Tessellator.
    Random var1 = new Random(10842L);
    List<double[]> out = new ArrayList<>();
    for (int var3 = 0; var3 < 1500; ++var3) {
      double var4 = (double)(var1.nextFloat() * 2.0F - 1.0F);
      double var6 = (double)(var1.nextFloat() * 2.0F - 1.0F);
      double var8 = (double)(var1.nextFloat() * 2.0F - 1.0F);
      double var10 = (double)(0.25F + var1.nextFloat() * 0.25F);
      double var12 = var4 * var4 + var6 * var6 + var8 * var8;
      if(var12 < 1.0D && var12 > 0.01D) {
        var12 = 1.0D / Math.sqrt(var12);
        var4 *= var12; var6 *= var12; var8 *= var12;
        double var14 = var4 * 100.0D, var16 = var6 * 100.0D, var18 = var8 * 100.0D;
        double var20 = Math.atan2(var4, var8);
        double var22 = Math.sin(var20), var24 = Math.cos(var20);
        double var26 = Math.atan2(Math.sqrt(var4 * var4 + var8 * var8), var6);
        double var28 = Math.sin(var26), var30 = Math.cos(var26);
        double var32 = var1.nextDouble() * Math.PI * 2.0D;
        double var34 = Math.sin(var32), var36 = Math.cos(var32);
        for(int var38 = 0; var38 < 4; ++var38) {
          double var39 = 0.0D;
          double var41 = (double)((var38 & 2) - 1) * var10;
          double var43 = (double)((var38 + 1 & 2) - 1) * var10;
          double var47 = var41 * var36 - var43 * var34;
          double var49 = var43 * var36 + var41 * var34;
          double var53 = var47 * var28 + var39 * var30;
          double var55 = var39 * var28 - var47 * var30;
          double var57 = var55 * var22 - var49 * var24;
          double var61 = var49 * var22 + var55 * var24;
          out.add(new double[]{var14 + var57, var16 + var53, var18 + var61});
        }
      }
    }
    double s0 = 0, s1 = 0, s2 = 0;
    for (double[] q : out) { s0 += q[0]; s1 += q[1]; s2 += q[2]; }
    StringBuilder sb = new StringBuilder("STARS " + out.size() + " " + s0 + " " + s1 + " " + s2);
    for (int i = 0; i < 8; i++) sb.append(" ").append(out.get(i)[0]).append(" ").append(out.get(i)[1]).append(" ").append(out.get(i)[2]);
    System.out.println(sb);
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
    sky();
  }
}
