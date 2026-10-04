import java.lang.reflect.*;
import net.minecraft.src.*;
public class DumpMat {
    public static void main(String[] a) throws Exception {
        for (Field fl : Material.class.getDeclaredFields()) {
            if (Modifier.isStatic(fl.getModifiers()) && fl.getType() == Material.class) {
                fl.setAccessible(true);
                Material m = (Material) fl.get(null);
                System.out.println("M|" + fl.getName() + "|" + m.materialMapColor.colorIndex + "|" + m.getIsLiquid() + "|" + m.isSolid() + "|" + m.getCanBlockGrass() + "|" + m.getIsSolid() + "|" + m.getBurning() + "|" + m.getIsGroundCover() + "|" + m.getIsTranslucent() + "|" + m.getIsHarvestable() + "|" + m.getMaterialMobility());
            }
        }
        for (Field fl : Block.class.getDeclaredFields()) {
            if (Modifier.isStatic(fl.getModifiers()) && StepSound.class.isAssignableFrom(fl.getType())) {
                fl.setAccessible(true);
                StepSound s = (StepSound) fl.get(null);
                System.out.println("S|" + fl.getName() + "|" + s.stepSoundDir() + "|" + s.getVolume() + "|" + s.getPitch() + "|" + s.getClass().getSimpleName());
            }
        }
        for (Field fl : MapColor.class.getDeclaredFields()) {
            if (Modifier.isStatic(fl.getModifiers()) && fl.getType() == MapColor.class) {
                fl.setAccessible(true);
                MapColor c = (MapColor) fl.get(null);
                System.out.println("C|" + fl.getName() + "|" + c.colorIndex + "|" + c.colorValue);
            }
        }
    }
}
