import net.minecraft.src.*;

/** Prints, for every block id, `Material.materialMapColor.colorIndex`, `getIsLiquid` and `Block.lightOpacity` (for `tools/gen_mapcolors.py`). */
public class MapTab {
    public static void main(String[] a) {
        for (int i = 0; i < 256; i++) {
            Block b = Block.blocksList[i];
            int color = b == null ? 0 : b.blockMaterial.materialMapColor.colorIndex;
            int liquid = b != null && b.blockMaterial.getIsLiquid() ? 1 : 0;
            System.out.println(i + " " + color + " " + liquid + " " + Block.lightOpacity[i]);
        }
    }
}
