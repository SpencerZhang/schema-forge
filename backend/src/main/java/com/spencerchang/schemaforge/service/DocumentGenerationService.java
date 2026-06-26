package com.spencerchang.schemaforge.service;

import cn.smallbun.screw.core.Configuration;
import cn.smallbun.screw.core.engine.EngineConfig;
import cn.smallbun.screw.core.engine.EngineFileType;
import cn.smallbun.screw.core.engine.EngineTemplateType;
import cn.smallbun.screw.core.execute.DocumentationExecute;
import cn.smallbun.screw.core.process.ProcessConfig;
import com.spencerchang.schemaforge.model.AppConfig;
import com.spencerchang.schemaforge.model.GenerateResult;
import com.zaxxer.hikari.HikariConfig;
import com.zaxxer.hikari.HikariDataSource;

import java.util.ArrayList;
import java.util.List;
import java.util.stream.Collectors;

public class DocumentGenerationService {

    public GenerateResult generate(AppConfig config) {
        validate(config);
        List<String> schemas = config.getScrew().getSchemas().stream()
                .filter(this::hasText)
                .map(String::trim)
                .collect(Collectors.toList());
        for (String schema : schemas) {
            generateSchemaDocument(config, schema);
        }
        return GenerateResult.success(schemas, config.getScrew().getEngine().getFileOutputDir());
    }

    private void generateSchemaDocument(AppConfig config, String schema) {
        HikariDataSource dataSource = createDataSource(config.getSpring().getDatasource(), schema);
        try {
            EngineConfig engineConfig = createEngineConfig(config.getScrew().getEngine(), schema);
            ProcessConfig processConfig = createProcessConfig();
            Configuration screwConfig = Configuration.builder()
                    .version("1.0.0")
                    .description("Database design document")
                    .dataSource(dataSource)
                    .engineConfig(engineConfig)
                    .produceConfig(processConfig)
                    .build();
            new DocumentationExecute(screwConfig).execute();
        } finally {
            dataSource.close();
        }
    }

    private HikariDataSource createDataSource(AppConfig.DataSource dataSourceConfig, String schema) {
        HikariConfig hikariConfig = new HikariConfig();
        hikariConfig.setDriverClassName(required(dataSourceConfig.getDriverClassName(), "spring.datasource.driver-class-name"));
        hikariConfig.setJdbcUrl(required(dataSourceConfig.getUrl(), "spring.datasource.url"));
        hikariConfig.setUsername(required(dataSourceConfig.getUsername(), "spring.datasource.username"));
        hikariConfig.setPassword(required(dataSourceConfig.getPassword(), "spring.datasource.password"));
        hikariConfig.setCatalog(schema);
        hikariConfig.addDataSourceProperty("useInformationSchema", "true");
        hikariConfig.setMinimumIdle(2);
        hikariConfig.setMaximumPoolSize(5);
        return new HikariDataSource(hikariConfig);
    }

    private EngineConfig createEngineConfig(AppConfig.Engine engine, String schema) {
        String fileName = hasText(engine.getFileName()) ? engine.getFileName().trim() : schema;
        return EngineConfig.builder()
                .fileOutputDir(required(engine.getFileOutputDir(), "screw.engine.file-output-dir"))
                .openOutputDir(engine.getOpenOutputDir() == null || engine.getOpenOutputDir())
                .fileType(parseEnum(valueOrDefault(engine.getFileType(), "HTML"), EngineFileType.class, "screw.engine.file-type"))
                .produceType(parseEnum(valueOrDefault(engine.getProduceType(), "freemarker"), EngineTemplateType.class, "screw.engine.produce-type"))
                .fileName(fileName)
                .build();
    }

    private ProcessConfig createProcessConfig() {
        ArrayList<String> ignoreTableName = new ArrayList<>();
        ignoreTableName.add("databasechangelog");
        ignoreTableName.add("databasechangeloglock");
        ArrayList<String> ignorePrefix = new ArrayList<>();
        ignorePrefix.add("test_");
        ArrayList<String> ignoreSuffix = new ArrayList<>();
        ignoreSuffix.add("_test");
        return ProcessConfig.builder()
                .designatedTableName(new ArrayList<>())
                .designatedTablePrefix(new ArrayList<>())
                .designatedTableSuffix(new ArrayList<>())
                .ignoreTableName(ignoreTableName)
                .ignoreTablePrefix(ignorePrefix)
                .ignoreTableSuffix(ignoreSuffix)
                .build();
    }

    private void validate(AppConfig config) {
        if (config == null || config.getSpring() == null || config.getSpring().getDatasource() == null) {
            throw new IllegalArgumentException("Missing datasource config.");
        }
        if (config.getScrew() == null || config.getScrew().getSchemas() == null
                || config.getScrew().getSchemas().stream().noneMatch(this::hasText)) {
            throw new IllegalArgumentException("Missing screw.schemas config.");
        }
        if (config.getScrew().getEngine() == null) {
            throw new IllegalArgumentException("Missing screw.engine config.");
        }
    }

    private String required(String value, String key) {
        if (!hasText(value)) {
            throw new IllegalArgumentException("Missing config: " + key);
        }
        return value.trim();
    }

    private String valueOrDefault(String value, String defaultValue) {
        return hasText(value) ? value.trim() : defaultValue;
    }

    private boolean hasText(String value) {
        return value != null && !value.trim().isEmpty();
    }

    private <T extends Enum<T>> T parseEnum(String value, Class<T> enumType, String key) {
        for (T enumValue : enumType.getEnumConstants()) {
            if (enumValue.name().equalsIgnoreCase(value)) {
                return enumValue;
            }
        }
        throw new IllegalArgumentException("Invalid config " + key + ": " + value);
    }
}
