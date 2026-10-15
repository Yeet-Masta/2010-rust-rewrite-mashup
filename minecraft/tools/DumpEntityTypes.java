import java.io.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.EntityDimensions;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.ai.attributes.Attribute;
import net.minecraft.world.entity.ai.attributes.AttributeSupplier;
import net.minecraft.world.entity.ai.attributes.DefaultAttributes;

// Writes minecraft/data/entity-types-26.3.json. Run like DumpModels:
//   javac -proc:none -cp "client.jar:libs/*" DumpEntityTypes.java
//   java -cp "client.jar:libs/*:." DumpEntityTypes entity-types-26.3.json

/** Each entity type's category, dimensions, fire immunity, tracking range and update interval, and a living type's default attribute base values. */
public class DumpEntityTypes {
    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        StringBuilder out = new StringBuilder("{");
        boolean first = true;
        for (EntityType<?> type : BuiltInRegistries.ENTITY_TYPE) {
            if (!first) out.append(',');
            first = false;
            EntityDimensions d = type.getDimensions();
            out.append('"').append(BuiltInRegistries.ENTITY_TYPE.getKey(type).getPath()).append("\":{")
                .append("\"category\":\"").append(type.getCategory().getName()).append('"')
                .append(",\"size\":[").append(d.width()).append(',').append(d.height()).append(',').append(d.eyeHeight()).append(']')
                .append(",\"fixed\":").append(d.fixed())
                .append(",\"fire_immune\":").append(type.fireImmune())
                .append(",\"tracking\":[").append(type.clientTrackingRange()).append(',').append(type.updateInterval()).append(']');
            if (DefaultAttributes.hasSupplier(type)) {
                AttributeSupplier supplier = DefaultAttributes.getSupplier((EntityType<? extends LivingEntity>) type);
                out.append(",\"attributes\":{");
                boolean firstAttribute = true;
                for (Holder<Attribute> attribute : BuiltInRegistries.ATTRIBUTE.asHolderIdMap()) {
                    if (!supplier.hasAttribute(attribute)) continue;
                    if (!firstAttribute) out.append(',');
                    firstAttribute = false;
                    out.append('"').append(attribute.unwrapKey().get().identifier().getPath()).append("\":").append(supplier.getBaseValue(attribute));
                }
                out.append('}');
            }
            out.append('}');
        }
        out.append('}');
        try (Writer w = new OutputStreamWriter(new FileOutputStream(args[0]), "UTF-8")) {
            w.write(out.toString());
        }
    }
}
