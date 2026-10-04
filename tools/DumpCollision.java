import java.lang.reflect.*;
import net.minecraft.src.*;

/** Dump bentuk tabrakan statis tiap blok. Null world dipakai; blok yang butuh World ditandai NEEDS_WORLD. */
public class DumpCollision {
    static String box(AxisAlignedBB b) {
        if (b == null) return "null";
        return b.minX + "," + b.minY + "," + b.minZ + "," + b.maxX + "," + b.maxY + "," + b.maxZ;
    }
    public static void main(String[] a) throws Exception {
        for (int i = 1; i < 256; i++) {
            Block b = Block.blocksList[i];
            if (b == null) continue;
            String coll, sel;
            try { coll = box(b.getCollisionBoundingBoxFromPool(null, 0, 0, 0)); } catch (Throwable t) { coll = "NEEDS_WORLD"; }
            try { sel = box(b.getSelectedBoundingBoxFromPool(null, 0, 0, 0)); } catch (Throwable t) { sel = "NEEDS_WORLD"; }
            long mask = 0; boolean ok = true;
            try {
                for (int m = 0; m < 16; m++) for (int f = 0; f < 2; f++)
                    if (b.canCollideCheck(m, f == 1)) mask |= 1L << (m * 2 + f);
            } catch (Throwable t) { ok = false; }
            System.out.println(i + "|" + b.getClass().getSimpleName() + "|" + coll + "|" + sel + "|" + (ok ? Long.toString(mask) : "NEEDS_WORLD") + "|" + b.isCollidable());
        }
    }
}
